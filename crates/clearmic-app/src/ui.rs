//! Fenetre principale : peripheriques, traitement, retour micro, soundboard, demarrage auto.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clearmic_dsp::param::{db_to_lin, lin_to_db};
use clearmic_dsp::player::{MAX_PADS, PadCommand};
use clearmic_dsp::settings::Settings;
use eframe::egui;

use crate::audio::{self, CommandQueue, Controls, Engine, EngineEvent, Role};
use crate::autostart;
use crate::config::{self, Config, MAX_PRESETS, PadConfig, VoicePreset};
use crate::devices::{self, Entry};
use crate::hotkeys::{Action, Binding, Combo, Hotkeys};
use crate::sound::{self, Bank};

const METER_FLOOR_DB: f32 = -60.0;
const SAVE_DELAY: Duration = Duration::from_millis(500);
/// Tentatives de demarrage automatique : les peripheriques peuvent etre prets apres l'ouverture
/// de session.
const AUTOSTART_RETRY: Duration = Duration::from_secs(3);
const AUTOSTART_ATTEMPTS: u32 = 20;
const AUDIO_EXTENSIONS: [&str; 4] = ["wav", "mp3", "ogg", "flac"];
/// Signature d'un pad pret pour le thread des raccourcis : (identifiant, gain, adresse du son).
type BankSig = Vec<(usize, u32, usize)>;

enum PadStatus {
    Loading,
    Ready(Arc<[f32]>),
    Missing,
}

struct Pad {
    id: usize,
    cfg: PadConfig,
    status: PadStatus,
}

type Decoded = (usize, Result<Arc<[f32]>, String>);

struct Autostart {
    attempts: u32,
    next: Instant,
}

pub struct ClearMicApp {
    host: cpal::Host,
    inputs: Vec<Entry>,
    outputs: Vec<Entry>,
    in_sel: Option<usize>,
    out_sel: Option<usize>,
    mon_sel: Option<usize>,
    cable_missing: bool,
    engine: Option<Engine>,
    error: Option<String>,
    notice: Option<String>,
    hold_in: f32,
    hold_out: f32,
    controls: Arc<Controls>,
    settings: Settings,
    monitor_on: bool,
    monitor_gain_db: f32,
    monitor_soundboard: bool,
    pads: Vec<Pad>,
    next_pad_id: usize,
    decoded_tx: Sender<Decoded>,
    decoded_rx: Receiver<Decoded>,
    picked_tx: Sender<Vec<PathBuf>>,
    picked_rx: Receiver<Vec<PathBuf>>,
    picking: bool,
    config_path: PathBuf,
    saved: Config,
    dirty_since: Option<Instant>,
    autostart_engine: bool,
    windows_autostart: bool,
    pending_start: Option<Autostart>,
    saved_in: Option<String>,
    saved_out: Option<String>,
    saved_mon: Option<String>,
    start_minimized: bool,
    low_latency: bool,
    queue: Arc<CommandQueue>,
    bank: Arc<Bank>,
    bank_sig: BankSig,
    hotkeys: Option<Hotkeys>,
    hotkey_failures: Vec<(Action, String)>,
    stop_all_hotkey: Option<String>,
    capturing: Option<Action>,
    presets: Vec<VoicePreset>,
    preset_name: String,
    tray: Option<crate::tray::Tray>,
}

impl ClearMicApp {
    pub fn set_tray(&mut self, tray: crate::tray::Tray) {
        self.tray = Some(tray);
    }

    pub fn new(start_minimized: bool) -> Self {
        let config_path = config::config_path();
        let loaded = config::load(&config_path);
        let cfg = loaded.config;
        let (decoded_tx, decoded_rx) = mpsc::channel();
        let (picked_tx, picked_rx) = mpsc::channel();
        let controls = Arc::new(Controls::new(cfg.settings, cfg.monitor_gain_db, cfg.monitor_soundboard));
        let queue = Arc::new(CommandQueue::new());
        let bank = Arc::new(Bank::default());
        let (hotkeys, hotkey_error) = match Hotkeys::spawn(queue.clone(), bank.clone()) {
            Ok(h) => (Some(h), None),
            Err(e) => (None, Some(format!("Raccourcis clavier indisponibles : {e}"))),
        };
        let mut app = Self {
            host: cpal::default_host(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            in_sel: None,
            out_sel: None,
            mon_sel: None,
            cable_missing: false,
            engine: None,
            error: hotkey_error,
            notice: loaded.notice,
            hold_in: 0.0,
            hold_out: 0.0,
            controls,
            settings: cfg.settings,
            monitor_on: false,
            monitor_gain_db: cfg.monitor_gain_db,
            monitor_soundboard: cfg.monitor_soundboard,
            pads: Vec::new(),
            next_pad_id: 0,
            decoded_tx,
            decoded_rx,
            picked_tx,
            picked_rx,
            picking: false,
            config_path,
            saved: cfg.clone(),
            dirty_since: None,
            autostart_engine: cfg.autostart_engine,
            windows_autostart: autostart::is_enabled(),
            pending_start: None,
            saved_in: cfg.input_device.clone(),
            saved_out: cfg.output_device.clone(),
            saved_mon: cfg.monitor_device.clone(),
            start_minimized,
            low_latency: cfg.low_latency,
            queue,
            bank,
            bank_sig: Vec::new(),
            hotkeys,
            hotkey_failures: Vec::new(),
            stop_all_hotkey: cfg.stop_all_hotkey.clone(),
            capturing: None,
            presets: cfg.presets.clone(),
            preset_name: String::new(),
            tray: None,
        };
        app.refresh_devices(true);
        let mut missing_devices = Vec::new();
        if let (Some(name), None) = (&cfg.input_device, app.saved_match(&app.inputs, &cfg.input_device)) {
            missing_devices.push(name.clone());
        }
        if let (Some(name), None) = (&cfg.output_device, app.saved_match(&app.outputs, &cfg.output_device)) {
            missing_devices.push(name.clone());
        }
        if !missing_devices.is_empty() {
            app.notice = Some(format!(
                "Périphérique enregistré introuvable ({}) : le périphérique par défaut de Windows est utilisé.",
                missing_devices.join(", ")
            ));
        }
        for p in cfg.pads {
            app.add_pad_from_config(p);
        }
        app.apply_hotkeys();
        if cfg.autostart_engine {
            app.pending_start = Some(Autostart { attempts: 0, next: Instant::now() });
        }
        app
    }

    fn saved_match(&self, list: &[Entry], saved: &Option<String>) -> Option<usize> {
        saved.as_ref().and_then(|n| list.iter().position(|e| &e.name == n))
    }

    /// Relit la liste des peripheriques. `use_defaults` : applique la config enregistree puis les
    /// defauts Windows ; sinon conserve les selections courantes par nom quand elles existent.
    fn refresh_devices(&mut self, use_defaults: bool) {
        let prev_in = self.in_sel.map(|i| self.inputs[i].name.clone());
        let prev_out = self.out_sel.map(|i| self.outputs[i].name.clone());
        let prev_mon = self.mon_sel.map(|i| self.outputs[i].name.clone());
        let (inputs, default_in) = devices::list_inputs(&self.host);
        let (outputs, default_out) = devices::list_outputs(&self.host);
        let in_names: Vec<String> = inputs.iter().map(|e| e.name.clone()).collect();
        let out_names: Vec<String> = outputs.iter().map(|e| e.name.clone()).collect();
        let find = |names: &[String], wanted: &Option<String>| {
            wanted.as_ref().and_then(|w| names.iter().position(|n| n == w))
        };
        let (want_in, want_out, want_mon) = if use_defaults {
            (self.saved_in.clone(), self.saved_out.clone(), self.saved_mon.clone())
        } else {
            (prev_in, prev_out, prev_mon)
        };
        self.in_sel = devices::pick_input(
            &in_names,
            want_in.as_deref(),
            default_in.map(|i| in_names[i].as_str()),
        );
        // La sortie du micro virtuel est preferee a la sortie par defaut (le haut-parleur).
        self.out_sel = find(&out_names, &want_out)
            .or_else(|| devices::cable_input_index(&out_names))
            .or_else(|| devices::default_index(&out_names, default_out.map(|i| out_names[i].as_str())));
        self.mon_sel = find(&out_names, &want_mon);
        self.cable_missing = devices::cable_input_index(&out_names).is_none();
        self.inputs = inputs;
        self.outputs = outputs;
    }

    fn start(&mut self) -> bool {
        self.refresh_devices(false);
        self.error = None;
        let (Some(i), Some(o)) = (self.in_sel, self.out_sel) else {
            self.error = Some("Choisissez un micro et une sortie.".into());
            return false;
        };
        if devices::is_virtual_cable(&self.inputs[i].name) {
            self.error = Some(format!(
                "« {} » ne peut pas servir de micro : il reprend la sortie de ClearMic. Choisissez votre vrai micro.",
                self.inputs[i].name
            ));
            return false;
        }
        let result = audio::start(
            &self.inputs[i].device,
            &self.inputs[i].name,
            &self.outputs[o].device,
            &self.outputs[o].name,
            self.controls.clone(),
            self.queue.clone(),
            self.low_latency,
        );
        match result {
            Ok(engine) => {
                if self.low_latency
                    && let Some(why) = &engine.low_latency_note
                {
                    self.notice = Some(format!("Sortie faible latence indisponible ({why}) : mode standard utilisé."));
                }
                self.engine = Some(engine);
                // Le retour micro est desactive a chaque demarrage (anti-larsen).
                self.monitor_on = false;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    fn stop(&mut self) {
        self.engine = None;
        self.monitor_on = false;
        self.hold_in = 0.0;
        self.hold_out = 0.0;
    }

    fn set_monitor(&mut self, on: bool) {
        if !on {
            if let Some(engine) = &mut self.engine {
                engine.stop_monitor();
            }
            self.monitor_on = false;
            return;
        }
        let (Some(o), Some(m)) = (self.out_sel, self.mon_sel) else {
            self.error = Some("Choisissez la sortie du retour micro (votre casque).".into());
            return;
        };
        if let Err(e) = devices::check_monitor_target(&self.outputs[m].name, &self.outputs[o].name) {
            self.error = Some(e);
            return;
        }
        let Some(engine) = &mut self.engine else { return };
        match engine.start_monitor(&self.outputs[m].device, &self.outputs[m].name) {
            Ok(()) => {
                self.monitor_on = true;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn poll_events(&mut self) {
        let Some(engine) = &self.engine else { return };
        let mut main_failure = None;
        let mut monitor_failure = None;
        while let Ok(EngineEvent::StreamFailed { role, device_lost, message }) = engine.events.try_recv() {
            let who = match role {
                Role::Input => "Le micro",
                Role::Output => "La sortie",
                Role::Monitor => "Le retour micro",
            };
            let name = match role {
                Role::Input => self.in_sel.map(|i| self.inputs[i].name.as_str()),
                Role::Output => self.out_sel.map(|i| self.outputs[i].name.as_str()),
                Role::Monitor => self.mon_sel.map(|i| self.outputs[i].name.as_str()),
            }
            .unwrap_or("?");
            let text = if device_lost {
                format!("{who} « {name} » a été débranché ou désactivé. Rebranchez-le puis réessayez.")
            } else {
                format!("{who} « {name} » : erreur audio ({message}).")
            };
            if role == Role::Monitor {
                monitor_failure = Some(text);
            } else {
                main_failure = Some(text);
            }
        }
        // Une panne du retour n'interrompt pas la sortie vers le micro virtuel.
        if let Some(text) = monitor_failure {
            if let Some(e) = &mut self.engine {
                e.stop_monitor();
            }
            self.monitor_on = false;
            self.error = Some(text);
        }
        if main_failure.is_some() {
            self.error = main_failure;
            self.stop();
        }
    }

    // ---- Soundboard ----

    fn add_pad_from_config(&mut self, cfg: PadConfig) {
        let id = self.next_pad_id;
        self.next_pad_id += 1;
        let path = PathBuf::from(&cfg.path);
        let status = if path.exists() {
            let tx = self.decoded_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((id, sound::decode_file(&path)));
            });
            PadStatus::Loading
        } else {
            PadStatus::Missing
        };
        self.pads.push(Pad { id, cfg, status });
    }

    /// Envoie au thread des raccourcis les sons prets et leur volume.
    fn sync_bank(&mut self) {
        let sig: BankSig = self
            .pads
            .iter()
            .filter_map(|p| match &p.status {
                PadStatus::Ready(d) => {
                    Some((p.id, db_to_lin(p.cfg.gain_db).to_bits(), Arc::as_ptr(d).cast::<f32>() as usize))
                }
                _ => None,
            })
            .collect();
        if sig == self.bank_sig {
            return;
        }
        let entries = self
            .pads
            .iter()
            .filter_map(|p| match &p.status {
                PadStatus::Ready(d) => Some((p.id, d.clone(), db_to_lin(p.cfg.gain_db))),
                _ => None,
            })
            .collect();
        self.bank.replace(entries);
        self.bank_sig = sig;
    }

    /// Enregistre les raccourcis des pads et du « tout arreter ». Pendant la saisie d'un raccourci,
    /// tout est libere pour que la combinaison tapee atteigne la fenetre.
    fn apply_hotkeys(&mut self) {
        let Some(hk) = &self.hotkeys else { return };
        let mut bindings = Vec::new();
        if self.capturing.is_none() {
            if let Some(c) = self.stop_all_hotkey.as_deref().and_then(|t| Combo::parse(t).ok()) {
                bindings.push(Binding { action: Action::StopAll, combo: c });
            }
            for p in &self.pads {
                if let Some(c) = p.cfg.hotkey.as_deref().and_then(|t| Combo::parse(t).ok()) {
                    bindings.push(Binding { action: Action::Pad(p.id), combo: c });
                }
            }
        }
        let failures = hk.set(bindings.clone());
        self.hotkey_failures = failures.into_iter().map(|(i, why)| (bindings[i].action, why)).collect();
    }

    /// Lit la combinaison tapee pendant la saisie d'un raccourci.
    fn poll_capture(&mut self, ctx: &egui::Context) {
        let Some(target) = self.capturing else { return };
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            let none = !(modifiers.ctrl || modifiers.alt || modifiers.shift);
            let chosen = if key == egui::Key::Escape {
                None
            } else if none && matches!(key, egui::Key::Backspace | egui::Key::Delete) {
                Some(None)
            } else {
                match Combo::new(key.name(), modifiers.ctrl, modifiers.alt, modifiers.shift, false) {
                    Ok(c) => Some(Some(c.to_string())),
                    Err(e) => {
                        self.error = Some(format!("Raccourci refusé : {e}."));
                        continue;
                    }
                }
            };
            if let Some(value) = chosen {
                match target {
                    Action::StopAll => self.stop_all_hotkey = value,
                    Action::Pad(id) => {
                        if let Some(p) = self.pads.iter_mut().find(|p| p.id == id) {
                            p.cfg.hotkey = value;
                        }
                    }
                }
                self.error = None;
            }
            self.capturing = None;
            self.apply_hotkeys();
            break;
        }
    }

    fn hotkey_failure(&self, action: Action) -> Option<&str> {
        self.hotkey_failures.iter().find(|(a, _)| *a == action).map(|(_, why)| why.as_str())
    }

    fn add_pad_file(&mut self, path: &Path) {
        if self.pads.len() >= MAX_PADS {
            self.error = Some(format!("Limite de {MAX_PADS} pads atteinte."));
            return;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()));
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("son").to_string();
        if !ext_ok {
            self.error = Some(format!("« {name} » n'est pas ajouté : format non pris en charge (WAV, MP3, OGG, FLAC)."));
            return;
        }
        self.add_pad_from_config(PadConfig { path: path.to_string_lossy().into_owned(), name, gain_db: 0.0, hotkey: None });
    }

    fn poll_sounds(&mut self) {
        while let Ok((id, result)) = self.decoded_rx.try_recv() {
            let Some(idx) = self.pads.iter().position(|p| p.id == id) else { continue };
            match result {
                Ok(data) => self.pads[idx].status = PadStatus::Ready(data),
                Err(reason) => {
                    // Un fichier invalide n'est pas garde dans la liste.
                    let pad = self.pads.remove(idx);
                    self.error = Some(format!("« {}» n'est pas ajouté : {reason}.", pad.cfg.name));
                }
            }
        }
        while let Ok(paths) = self.picked_rx.try_recv() {
            self.picking = false;
            for p in paths {
                self.add_pad_file(&p);
            }
        }
    }

    fn trigger(&mut self, idx: usize) {
        let pad = &self.pads[idx];
        let PadStatus::Ready(data) = &pad.status else { return };
        let cmd = PadCommand::Play { pad: pad.id, data: data.clone(), gain: db_to_lin(pad.cfg.gain_db) };
        let Some(engine) = &self.engine else {
            self.error = Some("Cliquez sur Démarrer pour envoyer un son dans le micro virtuel.".into());
            return;
        };
        if !engine.send(cmd) {
            self.error = Some("Trop de sons déclenchés d'un coup : réessayez.".into());
        }
    }

    fn remove_pad(&mut self, idx: usize) {
        let pad = self.pads.remove(idx);
        if let Some(engine) = &self.engine {
            engine.send(PadCommand::Stop(pad.id));
        }
        self.apply_hotkeys();
    }

    fn stop_pad(&mut self, idx: usize) {
        if let Some(engine) = &self.engine {
            engine.send(PadCommand::Stop(self.pads[idx].id));
        }
    }

    // ---- Configuration ----

    fn snapshot(&self) -> Config {
        Config {
            version: config::VERSION,
            input_device: self.in_sel.map(|i| self.inputs[i].name.clone()).or_else(|| self.saved.input_device.clone()),
            output_device: self.out_sel.map(|i| self.outputs[i].name.clone()).or_else(|| self.saved.output_device.clone()),
            monitor_device: self.mon_sel.map(|i| self.outputs[i].name.clone()).or_else(|| self.saved.monitor_device.clone()),
            monitor_gain_db: self.monitor_gain_db,
            monitor_soundboard: self.monitor_soundboard,
            settings: self.settings,
            pads: self.pads.iter().map(|p| p.cfg.clone()).collect(),
            autostart_engine: self.autostart_engine,
            low_latency: self.low_latency,
            stop_all_hotkey: self.stop_all_hotkey.clone(),
            presets: self.presets.clone(),
        }
    }

    fn save_if_needed(&mut self, force: bool) {
        let cfg = self.snapshot();
        if cfg == self.saved {
            self.dirty_since = None;
            return;
        }
        let since = *self.dirty_since.get_or_insert_with(Instant::now);
        if force || since.elapsed() >= SAVE_DELAY {
            match config::save(&self.config_path, &cfg) {
                Ok(()) => self.saved = cfg,
                Err(e) => self.error = Some(format!("Sauvegarde des réglages impossible : {e}")),
            }
            self.dirty_since = None;
        }
    }

    fn drive_autostart(&mut self) {
        let Some(state) = &mut self.pending_start else { return };
        if self.engine.is_some() {
            self.pending_start = None;
            return;
        }
        if Instant::now() < state.next {
            return;
        }
        state.attempts += 1;
        let attempts = state.attempts;
        state.next = Instant::now() + AUTOSTART_RETRY;
        let ok = self.start();
        if ok || attempts >= AUTOSTART_ATTEMPTS {
            self.pending_start = None;
        } else {
            // Les echecs transitoires (peripheriques pas encore prets) ne sont pas affiches.
            self.error = None;
        }
    }
}

fn combo(ui: &mut egui::Ui, label: &str, entries: &[Entry], sel: &mut Option<usize>) {
    let text = sel.map(|i| entries[i].name.as_str()).unwrap_or("Aucun périphérique");
    egui::ComboBox::from_label(label)
        .selected_text(text)
        .width(340.0)
        .show_ui(ui, |ui| {
            for (i, e) in entries.iter().enumerate() {
                ui.selectable_value(sel, Some(i), &e.name);
            }
        });
}

fn meter(ui: &mut egui::Ui, label: &str, level: f32) {
    let db = lin_to_db(level).max(METER_FLOOR_DB);
    let frac = (db - METER_FLOOR_DB) / -METER_FLOOR_DB;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::ProgressBar::new(frac).desired_width(260.0).text(format!("{db:.1} dBFS")));
    });
}

fn slider(ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) {
    ui.add(egui::Slider::new(value, range).text(label).suffix(suffix));
}

fn log_slider(ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) {
    ui.add(egui::Slider::new(value, range).logarithmic(true).text(label).suffix(suffix));
}

/// « 3:07 » pour un son de 187 s.
fn format_duration(frames: usize) -> String {
    let secs = frames / 48_000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

const WARNING: egui::Color32 = egui::Color32::from_rgb(230, 160, 40);
const DANGER: egui::Color32 = egui::Color32::from_rgb(230, 70, 70);

impl eframe::App for ClearMicApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.start_minimized {
            self.start_minimized = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        let ctx = ui.ctx().clone();
        if let Some(tray) = &self.tray {
            tray.hide_if_minimized(&ctx);
        }
        self.poll_events();
        self.poll_sounds();
        self.poll_capture(&ctx);
        self.drive_autostart();

        let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            let path = f.path().to_path_buf();
            self.add_pad_file(&path);
        }

        let running = self.engine.is_some();
        let mut settings = self.settings;
        let mut capture_changed = false;

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.heading("ClearMic");
                ui.add_space(4.0);

                if self.cable_missing {
                    ui.colored_label(
                        WARNING,
                        "Attention : VB-CABLE est absent (aucune sortie « CABLE Input »). \
                         Installez-le depuis vb-audio.com/Cable pour obtenir un micro virtuel.",
                    );
                }
                if let Some(msg) = &self.notice {
                    ui.colored_label(WARNING, msg);
                }
                if let Some(err) = &self.error {
                    ui.colored_label(DANGER, format!("Erreur : {err}"));
                }

                ui.add_enabled_ui(!running, |ui| {
                    if self.inputs.is_empty() {
                        ui.label("Aucun périphérique de capture trouvé.");
                    }
                    if self.outputs.is_empty() {
                        ui.label("Aucun périphérique de rendu trouvé.");
                    }
                    combo(ui, "Micro", &self.inputs, &mut self.in_sel);
                    combo(ui, "Sortie (micro virtuel)", &self.outputs, &mut self.out_sel);
                    if ui.button("Actualiser les périphériques").clicked() {
                        self.refresh_devices(false);
                    }
                });

                ui.add_space(6.0);
                let can_start = self.in_sel.is_some() && self.out_sel.is_some();
                ui.horizontal(|ui| {
                    if running {
                        if ui.button("Arrêter").clicked() {
                            self.stop();
                        }
                    } else if ui.add_enabled(can_start, egui::Button::new("Démarrer")).clicked() {
                        self.pending_start = None;
                        self.start();
                    }
                    ui.label(if self.engine.is_some() { "État : en marche" } else { "État : arrêté" });
                });

                if let Some(engine) = &self.engine {
                    let s = &engine.stats;
                    self.hold_in = s.in_peak.get().max(self.hold_in * 0.9);
                    self.hold_out = s.out_peak.get().max(self.hold_out * 0.9);
                    ui.add_space(6.0);
                    meter(ui, "Entrée ", self.hold_in);
                    meter(ui, "Sortie ", self.hold_out);
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Latence estimée : {:.0} ms (hors tampon interne du pilote VB-CABLE)",
                            engine.latency_ms(settings.denoise_on)
                        ));
                        if self.controls.meters.clip.load(Ordering::Relaxed) {
                            ui.colored_label(DANGER, "CLIP");
                        }
                    });
                    match (engine.low_latency_period, self.low_latency) {
                        (Some(p), _) => {
                            ui.label(format!("Sortie faible latence active (blocs de {:.1} ms).", p as f32 * 1000.0 / 48_000.0));
                        }
                        (None, true) => {
                            ui.colored_label(WARNING, "Sortie faible latence indisponible sur ce périphérique : mode standard. Choisissez « CABLE Input » comme sortie.");
                        }
                        (None, false) => {}
                    }
                    ui.label(format!(
                        "Coupures : {}   Débordements : {}",
                        s.underruns.load(Ordering::Relaxed),
                        s.overruns.load(Ordering::Relaxed)
                    ));
                }

                ui.add_space(8.0);
                egui::CollapsingHeader::new("Débruitage").default_open(true).show(ui, |ui| {
                    ui.checkbox(&mut settings.denoise_on, "Activer la suppression de bruit (RNNoise)");
                    let mut pct = settings.denoise_amount * 100.0;
                    slider(ui, "Intensité", &mut pct, 0.0..=100.0, " %");
                    settings.denoise_amount = pct / 100.0;
                    ui.label("Ajoute environ 10 ms de latence quand activé.");
                });

                egui::CollapsingHeader::new("Gain, boost et canaux").default_open(true).show(ui, |ui| {
                    slider(ui, "Gain d'entrée", &mut settings.input_gain_db, -20.0..=20.0, " dB");
                    slider(ui, "Boost", &mut settings.boost_db, 0.0..=30.0, " dB");
                    if settings.boost_db > 12.0 || settings.input_gain_db > 12.0 {
                        ui.colored_label(
                            WARNING,
                            "Un fort gain amplifie aussi le bruit résiduel et durcit la voix. Préférez un gain \
                             modéré et le compresseur avec gain de compensation.",
                        );
                    }
                    ui.checkbox(&mut settings.limiter_on, "Limiteur de sortie (-1 dBFS)");
                    ui.checkbox(&mut settings.mono, "Mode mono (L = R)");
                    ui.add_enabled_ui(!settings.mono, |ui| {
                        slider(ui, "Balance", &mut settings.balance, -100.0..=100.0, "");
                        slider(ui, "Largeur stéréo", &mut settings.width, 0.0..=200.0, " %");
                    });
                });

                egui::CollapsingHeader::new("Égaliseur").show(ui, |ui| {
                    ui.checkbox(&mut settings.hpf_on, "Filtre passe-haut");
                    ui.add_enabled_ui(settings.hpf_on, |ui| {
                        log_slider(ui, "Coupure", &mut settings.hpf_freq, 20.0..=20_000.0, " Hz");
                    });
                    ui.separator();
                    ui.checkbox(&mut settings.eq_on, "Activer les bandes");
                    ui.add_enabled_ui(settings.eq_on, |ui| {
                        const NAMES: [&str; 5] = ["Graves (étagère)", "Bas-médiums", "Médiums", "Hauts-médiums", "Aigus (étagère)"];
                        for (i, band) in settings.eq.iter_mut().enumerate() {
                            ui.label(NAMES[i]);
                            log_slider(ui, "Fréquence", &mut band.freq, 20.0..=20_000.0, " Hz");
                            slider(ui, "Gain", &mut band.gain_db, -12.0..=12.0, " dB");
                            slider(ui, "Q", &mut band.q, 0.1..=10.0, "");
                        }
                    });
                });

                egui::CollapsingHeader::new("Compresseur").show(ui, |ui| {
                    ui.checkbox(&mut settings.comp_on, "Activer le compresseur");
                    ui.add_enabled_ui(settings.comp_on, |ui| {
                        let c = &mut settings.comp;
                        slider(ui, "Seuil", &mut c.threshold_db, -60.0..=0.0, " dB");
                        slider(ui, "Ratio", &mut c.ratio, 1.0..=20.0, " :1");
                        slider(ui, "Attaque", &mut c.attack_ms, 1.0..=100.0, " ms");
                        slider(ui, "Relâchement", &mut c.release_ms, 10.0..=1000.0, " ms");
                        slider(ui, "Coude", &mut c.knee_db, 0.0..=12.0, " dB");
                        slider(ui, "Gain de compensation", &mut c.makeup_db, 0.0..=24.0, " dB");
                    });
                    let gr = self.controls.meters.comp_gr_db.get();
                    ui.horizontal(|ui| {
                        ui.label("Réduction de gain");
                        ui.add(egui::ProgressBar::new((gr / 24.0).clamp(0.0, 1.0)).desired_width(200.0).text(format!("{gr:.1} dB")));
                    });
                });

                egui::CollapsingHeader::new("Retour micro").show(ui, |ui| {
                    ui.label("Pour vous entendre dans votre casque. Désactivé à chaque lancement.");
                    ui.add_enabled_ui(!self.monitor_on, |ui| {
                        combo(ui, "Sortie du retour (casque)", &self.outputs, &mut self.mon_sel);
                    });
                    let mut on = self.monitor_on;
                    if ui.add_enabled(running, egui::Checkbox::new(&mut on, "Activer le retour micro")).changed() {
                        self.set_monitor(on);
                    }
                    if !running {
                        ui.label("Démarrez le traitement pour activer le retour.");
                    }
                    slider(ui, "Volume du retour", &mut self.monitor_gain_db, -60.0..=0.0, " dB");
                    self.controls.monitor_gain_db.set(self.monitor_gain_db);
                    ui.checkbox(&mut self.monitor_soundboard, "Entendre le soundboard");
                    self.controls.monitor_soundboard.store(self.monitor_soundboard, Ordering::Relaxed);
                    if let Some(engine) = &self.engine
                        && engine.monitor_running()
                    {
                        ui.label(format!(
                            "Latence du retour : {:.0} ms   Coupures du retour : {}",
                            engine.monitor_latency_ms(settings.denoise_on),
                            engine.stats.mon_underruns.load(Ordering::Relaxed)
                        ));
                    }
                });

                egui::CollapsingHeader::new("Presets de voix").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.preset_name).hint_text("Nom du preset").desired_width(160.0));
                        let name = self.preset_name.trim().to_string();
                        let exists = self.presets.iter().any(|p| p.name == name);
                        let can_save = !name.is_empty() && (exists || self.presets.len() < MAX_PRESETS);
                        let label = if exists { "Remplacer par les réglages actuels" } else { "Enregistrer les réglages actuels" };
                        if ui.add_enabled(can_save, egui::Button::new(label)).clicked() {
                            let preset = VoicePreset { name: name.clone(), settings };
                            match self.presets.iter_mut().find(|p| p.name == name) {
                                Some(p) => *p = preset,
                                None => self.presets.push(preset),
                            }
                        }
                    });
                    if self.presets.is_empty() {
                        ui.label("Aucun preset : réglez la voix, donnez un nom, puis enregistrez.");
                    }
                    let mut load = None;
                    let mut delete = None;
                    egui::Grid::new("presets").num_columns(3).striped(true).show(ui, |ui| {
                        for (i, p) in self.presets.iter().enumerate() {
                            ui.label(&p.name);
                            if ui.button("Charger").clicked() {
                                load = Some(i);
                            }
                            if ui.button("Supprimer").clicked() {
                                delete = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(i) = load {
                        settings = self.presets[i].settings;
                        self.preset_name = self.presets[i].name.clone();
                    }
                    if let Some(i) = delete {
                        self.presets.remove(i);
                    }
                });

                egui::CollapsingHeader::new("Soundboard").default_open(true).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.picking, egui::Button::new("Ajouter des sons")).clicked() {
                            self.picking = true;
                            let tx = self.picked_tx.clone();
                            std::thread::spawn(move || {
                                let paths = rfd::FileDialog::new()
                                    .add_filter("Audio", &AUDIO_EXTENSIONS)
                                    .pick_files()
                                    .unwrap_or_default();
                                let _ = tx.send(paths);
                            });
                        }
                        if ui.button("Tout arrêter").clicked()
                            && let Some(engine) = &self.engine
                        {
                            engine.send(PadCommand::StopAll);
                        }
                        let stop_text = match (self.capturing, &self.stop_all_hotkey) {
                            (Some(Action::StopAll), _) => "Appuyez sur la combinaison...".to_string(),
                            (_, Some(h)) => format!("Raccourci : {h}"),
                            (_, None) => "Raccourci : aucun".to_string(),
                        };
                        if ui.button(stop_text).clicked() {
                            self.capturing = Some(Action::StopAll);
                            self.hotkey_failures.clear();
                            capture_changed = true;
                        }
                        if let Some(why) = self.hotkey_failure(Action::StopAll) {
                            ui.colored_label(DANGER, why);
                        }
                    });
                    slider(ui, "Volume global", &mut settings.soundboard_db, -60.0..=6.0, " dB");
                    if self.capturing.is_some() {
                        ui.colored_label(WARNING, "Tapez la combinaison voulue (Échap : annuler, Retour arrière : effacer).");
                    }
                    if self.pads.is_empty() {
                        ui.label("Glissez des fichiers audio ici (WAV, MP3, OGG, FLAC, durée illimitée).");
                    }
                    let mut remove = None;
                    let mut play = None;
                    let mut stop = None;
                    let mut capture = None;
                    egui::Grid::new("pads").num_columns(6).striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
                        for h in ["Nom", "Durée", "Volume", "Raccourci", "", ""] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for i in 0..self.pads.len() {
                            let id = self.pads[i].id;
                            let capturing_this = self.capturing == Some(Action::Pad(id));
                            let key_text = if capturing_this {
                                "Appuyez sur la combinaison...".to_string()
                            } else {
                                self.pads[i].cfg.hotkey.clone().unwrap_or_else(|| "Aucun".to_string())
                            };
                            let failure = self.hotkey_failure(Action::Pad(id)).map(str::to_string);
                            let pad = &mut self.pads[i];
                            ui.add(egui::TextEdit::singleline(&mut pad.cfg.name).id_salt(("pad-name", id)).desired_width(150.0));
                            match &pad.status {
                                PadStatus::Loading => {
                                    ui.label("Chargement...");
                                    ui.label("");
                                }
                                PadStatus::Missing => {
                                    ui.colored_label(WARNING, "Introuvable");
                                    ui.label("");
                                }
                                PadStatus::Ready(d) => {
                                    ui.label(format_duration(d.len() / 2));
                                    ui.add(egui::Slider::new(&mut pad.cfg.gain_db, -60.0..=6.0).suffix(" dB"));
                                }
                            }
                            ui.horizontal(|ui| {
                                if ui.button(key_text).clicked() {
                                    capture = Some(id);
                                }
                                if let Some(why) = failure {
                                    ui.colored_label(DANGER, why);
                                }
                            });
                            let ready = matches!(pad.status, PadStatus::Ready(_));
                            ui.horizontal(|ui| {
                                if ui.add_enabled(ready, egui::Button::new("Lire")).clicked() {
                                    play = Some(i);
                                }
                                if ui.add_enabled(ready, egui::Button::new("Stop")).clicked() {
                                    stop = Some(i);
                                }
                            });
                            if ui.small_button("Retirer").clicked() {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(id) = capture {
                        self.capturing = Some(Action::Pad(id));
                        self.hotkey_failures.clear();
                        capture_changed = true;
                    }
                    if let Some(i) = play {
                        self.trigger(i);
                    }
                    if let Some(i) = stop {
                        self.stop_pad(i);
                    }
                    if let Some(i) = remove {
                        self.remove_pad(i);
                    }
                });

                egui::CollapsingHeader::new("Latence").show(ui, |ui| {
                    ui.checkbox(
                        &mut self.low_latency,
                        "Sortie faible latence (WASAPI, blocs de 2,7 ms si le pilote le permet)",
                    );
                    ui.label("S'applique au prochain démarrage. En cas de coupures, décochez cette case.");
                    ui.label("Le micro lui-même livre des blocs de 10 ms, et le débruitage ajoute 10 ms : c'est le plancher.");
                });

                egui::CollapsingHeader::new("Démarrage automatique").show(ui, |ui| {
                    let mut win = self.windows_autostart;
                    if ui.checkbox(&mut win, "Lancer ClearMic avec Windows (fenêtre réduite)").changed() {
                        match autostart::set_enabled(win) {
                            Ok(()) => self.windows_autostart = win,
                            Err(e) => self.error = Some(e),
                        }
                    }
                    ui.checkbox(&mut self.autostart_engine, "Démarrer le traitement automatiquement au lancement");
                });

                if let Some(path) = Some(&self.config_path) {
                    ui.add_space(8.0);
                    ui.weak(format!("Réglages : {}", path.display()));
                }
            });
        });

        if settings != self.settings {
            self.settings = settings;
        }
        if capture_changed {
            self.apply_hotkeys();
        }
        self.sync_bank();
        self.controls.settings.set(self.settings);
        self.save_if_needed(false);

        let loading = self.pads.iter().any(|p| matches!(p.status, PadStatus::Loading));
        if running {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        } else if loading || self.pending_start.is_some() || self.picking || self.dirty_since.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_if_needed(true);
    }
}
