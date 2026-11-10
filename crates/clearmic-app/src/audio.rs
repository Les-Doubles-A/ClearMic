//! Moteur audio : capture, file de decouplage avec correction de derive, traitement, sortie
//! vers le micro virtuel, retour micro optionnel (second flux de sortie) et soundboard.
//!
//! Les rappels audio n'allouent pas, ne prennent aucun verrou bloquant et n'ecrivent rien sur
//! disque.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use clearmic_dsp::drift::{
    FillAction, FillController, LOW_LATENCY_PRIME_EXTRA, LOW_LATENCY_SLACK, LOW_LATENCY_TARGET,
};
use clearmic_dsp::latency::estimate_latency_ms;
use clearmic_dsp::param::{AtomicF32, db_to_lin};
use clearmic_dsp::player::PadCommand;
use clearmic_dsp::processor::Processor;
use clearmic_dsp::settings::{Meters, Settings, SettingsHandle};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, Device, ErrorKind, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{CachingCons, CachingProd, HeapCons, HeapProd, HeapRb};

use crate::devices::validate_format;
use crate::lowlat::{self, LowLatencyOutput};

/// Marge visee dans la file apres lecture : une trame de capture (10 ms) plus 2 ms de gigue.
/// En dessous, la phase relative des deux horloges provoque periodiquement des coupures
/// (voir les simulations de `clearmic_dsp::drift`).
const TARGET_FILL: usize = 480 + 96;
const SLACK: usize = 96;
const WINDOW_BLOCKS: usize = 50;
const RING_FRAMES: usize = 9_600;
/// Taille maximale d'un bloc traite dans un rappel (trames).
const MAX_FRAMES: usize = 16_384;
const COMMAND_QUEUE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Input,
    Output,
    Monitor,
}

#[derive(Debug)]
pub enum EngineEvent {
    /// Un flux s'est arrete (peripherique debranche ou erreur).
    StreamFailed { role: Role, device_lost: bool, message: String },
}

/// Compteurs et niveaux partages avec l'interface (lecture seule cote interface).
#[derive(Default)]
pub struct Stats {
    pub in_peak: AtomicF32,
    pub out_peak: AtomicF32,
    pub in_chunk: AtomicU32,
    pub out_chunk: AtomicU32,
    pub fill_frames: AtomicU32,
    pub underruns: AtomicU64,
    pub overruns: AtomicU64,
    pub mon_chunk: AtomicU32,
    pub mon_fill: AtomicU32,
    pub mon_underruns: AtomicU64,
}

/// File des commandes du soundboard, alimentee par l'interface et par les raccourcis clavier,
/// videe par le rappel de sortie (qui ne bloque jamais : si le verrou est pris, il repasse au bloc
/// suivant).
pub struct CommandQueue {
    active: AtomicBool,
    items: Mutex<VecDeque<PadCommand>>,
}

impl CommandQueue {
    pub fn new() -> Self {
        Self { active: AtomicBool::new(false), items: Mutex::new(VecDeque::with_capacity(COMMAND_QUEUE)) }
    }

    /// Faux si le moteur est arrete ou si la file est pleine.
    pub fn push(&self, cmd: PadCommand) -> bool {
        if !self.active.load(Ordering::Acquire) {
            return false;
        }
        match self.items.lock() {
            Ok(mut q) if q.len() < COMMAND_QUEUE => {
                q.push_back(cmd);
                true
            }
            _ => false,
        }
    }

    fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Release);
        if !on && let Ok(mut q) = self.items.lock() {
            q.clear();
        }
    }

    fn drain(&self, mut apply: impl FnMut(PadCommand)) {
        if let Ok(mut q) = self.items.try_lock() {
            while let Some(cmd) = q.pop_front() {
                apply(cmd);
            }
        }
    }
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Reglages vivants, partages entre l'interface et les rappels audio. Survit aux redemarrages.
pub struct Controls {
    pub settings: SettingsHandle,
    pub meters: Arc<Meters>,
    pub monitor_gain_db: AtomicF32,
    pub monitor_soundboard: AtomicBool,
}

impl Controls {
    pub fn new(settings: Settings, monitor_gain_db: f32, monitor_soundboard: bool) -> Self {
        Self {
            settings: SettingsHandle::new(settings),
            meters: Arc::new(Meters::default()),
            monitor_gain_db: AtomicF32::new(monitor_gain_db),
            monitor_soundboard: AtomicBool::new(monitor_soundboard),
        }
    }
}

/// Flux de sortie principal : cpal (periode du moteur) ou WASAPI IAudioClient3 (periode minimale).
enum OutputHandle {
    Standard(#[allow(dead_code)] Stream),
    LowLatency(#[allow(dead_code)] LowLatencyOutput),
}

pub struct Engine {
    _input: Stream,
    _output: OutputHandle,
    /// Periode de la sortie en trames si le mode faible latence est actif.
    pub low_latency_period: Option<u32>,
    /// Pourquoi le mode faible latence n'est pas actif, quand il etait demande.
    pub low_latency_note: Option<String>,
    monitor: Option<Stream>,
    monitor_rb: Arc<HeapRb<f32>>,
    monitor_active: Arc<AtomicBool>,
    commands: Arc<CommandQueue>,
    tx: Sender<EngineEvent>,
    pub stats: Arc<Stats>,
    pub events: Receiver<EngineEvent>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.commands.set_active(false);
    }
}

impl Engine {
    /// Latence estimee en ms (voir `clearmic_dsp::latency`).
    pub fn latency_ms(&self, denoise_active: bool) -> f32 {
        estimate_latency_ms(
            self.stats.in_chunk.load(Ordering::Relaxed),
            self.stats.out_chunk.load(Ordering::Relaxed),
            self.stats.fill_frames.load(Ordering::Relaxed) as f32,
            denoise_active,
        )
    }

    /// Latence estimee du retour micro : meme chaine jusqu'a la sortie principale, puis la file et
    /// la trame du second peripherique.
    pub fn monitor_latency_ms(&self, denoise_active: bool) -> f32 {
        let s = &self.stats;
        estimate_latency_ms(
            s.in_chunk.load(Ordering::Relaxed) + s.out_chunk.load(Ordering::Relaxed),
            s.mon_chunk.load(Ordering::Relaxed),
            (s.fill_frames.load(Ordering::Relaxed) + s.mon_fill.load(Ordering::Relaxed)) as f32,
            denoise_active,
        )
    }

    pub fn monitor_running(&self) -> bool {
        self.monitor.is_some()
    }

    /// Declenche ou arrete un pad. Faux si la file de commandes est pleine.
    pub fn send(&self, cmd: PadCommand) -> bool {
        self.commands.push(cmd)
    }

    pub fn start_monitor(&mut self, device: &Device, name: &str) -> Result<(), String> {
        self.stop_monitor();
        let cfg = device
            .default_output_config()
            .map_err(|e| format!("Sortie « {name} » inutilisable : {}", describe(&e)))?;
        validate_format(name, cfg.sample_rate(), cpal::SampleFormat::F32)?;
        // Vide les restes d'une session precedente avant de rebrancher le flux.
        let mut cons: HeapCons<f32> = CachingCons::new(self.monitor_rb.clone());
        cons.clear();
        let mut reader = Reader::new(cons);
        let stats = self.stats.clone();
        let tx = self.tx.clone();
        let stream = device
            .build_output_stream::<f32, _, _>(
                StreamConfig {
                    channels: 2,
                    sample_rate: crate::devices::REQUIRED_RATE,
                    buffer_size: BufferSize::Default,
                },
                move |out: &mut [f32], _| {
                    let frames = out.len() / 2;
                    stats.mon_chunk.store(frames as u32, Ordering::Relaxed);
                    if frames == 0 || frames > MAX_FRAMES {
                        out.fill(0.0);
                        return;
                    }
                    match reader.read(frames) {
                        Read::Priming => out.fill(0.0),
                        Read::Underrun => {
                            out.fill(0.0);
                            stats.mon_underruns.fetch_add(1, Ordering::Relaxed);
                        }
                        Read::Block => {
                            for (o, s) in out.iter_mut().zip(reader.block(frames)) {
                                *o = s.clamp(-1.0, 1.0);
                            }
                            stats.mon_fill.store(reader.fill_frames() as u32, Ordering::Relaxed);
                        }
                    }
                },
                move |e| {
                    if is_fatal(&e) {
                        let _ = tx.send(EngineEvent::StreamFailed {
                            role: Role::Monitor,
                            device_lost: e.kind() == ErrorKind::DeviceNotAvailable,
                            message: describe(&e),
                        });
                    }
                },
                None,
            )
            .map_err(|e| format!("Impossible d'ouvrir la sortie « {name} » : {}", describe(&e)))?;
        stream
            .play()
            .map_err(|e| format!("Démarrage du retour micro impossible : {}", describe(&e)))?;
        self.monitor = Some(stream);
        self.monitor_active.store(true, Ordering::Release);
        Ok(())
    }

    pub fn stop_monitor(&mut self) {
        self.monitor_active.store(false, Ordering::Release);
        self.monitor = None;
        self.stats.mon_fill.store(0, Ordering::Relaxed);
        self.stats.mon_chunk.store(0, Ordering::Relaxed);
    }
}

fn describe(e: &cpal::Error) -> String {
    e.to_string()
}

/// Les avertissements (priorite temps reel refusee, re-routage automatique, glitch de tampon)
/// n'arretent pas le flux.
fn is_fatal(e: &cpal::Error) -> bool {
    !matches!(e.kind(), ErrorKind::RealtimeDenied | ErrorKind::DeviceChanged | ErrorKind::Xrun)
}

enum Read {
    Priming,
    Underrun,
    Block,
}

/// Lecteur de file avec correction de derive : produit des blocs stereo entrelaces.
struct Reader {
    cons: HeapCons<f32>,
    ctl: FillController,
    scratch: Vec<f32>,
}

impl Reader {
    fn new(cons: HeapCons<f32>) -> Self {
        Self {
            cons,
            ctl: FillController::new(TARGET_FILL, SLACK, WINDOW_BLOCKS),
            scratch: vec![0f32; MAX_FRAMES * 2],
        }
    }

    fn read(&mut self, frames: usize) -> Read {
        let available = self.cons.occupied_len() / 2;
        match self.ctl.decide(available, frames) {
            FillAction::Priming => Read::Priming,
            FillAction::Underrun => Read::Underrun,
            FillAction::Play { skip, repeat } => {
                self.cons.skip(skip * 2);
                let read = frames - repeat;
                self.cons.pop_slice(&mut self.scratch[..read * 2]);
                if repeat > 0 && read > 0 {
                    let (l, r) = (self.scratch[2 * read - 2], self.scratch[2 * read - 1]);
                    for i in read..frames {
                        self.scratch[2 * i] = l;
                        self.scratch[2 * i + 1] = r;
                    }
                }
                Read::Block
            }
        }
    }

    fn block(&mut self, frames: usize) -> &mut [f32] {
        &mut self.scratch[..frames * 2]
    }

    fn fill_frames(&self) -> usize {
        self.cons.occupied_len() / 2
    }
}

/// Tout ce que fait le rappel de sortie : lecture de la file, chaine de traitement, retour micro.
struct OutputChain {
    reader: Reader,
    proc: Processor,
    settings: Settings,
    controls: Arc<Controls>,
    commands: Arc<CommandQueue>,
    stats: Arc<Stats>,
    mon_prod: HeapProd<f32>,
    mon_scratch: Vec<f32>,
    mon_active: Arc<AtomicBool>,
    mon_gain_prev: f32,
    /// Remplissage moyen de la file apres lecture (lisse : la file oscille entre deux blocs).
    fill_avg: f32,
}

impl OutputChain {
    /// Etat vide qui remplace une chaine confiee a un autre thread.
    fn placeholder() -> Self {
        let (_, cons) = HeapRb::<f32>::new(2).split();
        let rb = Arc::new(HeapRb::<f32>::new(2));
        Self {
            reader: Reader { cons, ctl: FillController::new(0, 0, 1), scratch: Vec::new() },
            proc: Processor::new(Arc::new(Meters::default())),
            settings: Settings::default(),
            controls: Arc::new(Controls::new(Settings::default(), 0.0, false)),
            commands: Arc::new(CommandQueue::new()),
            stats: Arc::new(Stats::default()),
            mon_prod: CachingProd::new(rb),
            mon_scratch: Vec::new(),
            mon_active: Arc::new(AtomicBool::new(false)),
            mon_gain_prev: 1.0,
            fill_avg: 0.0,
        }
    }

    fn set_low_latency(&mut self, low: bool) {
        self.reader.ctl = if low {
            FillController::new(LOW_LATENCY_TARGET, LOW_LATENCY_SLACK, WINDOW_BLOCKS)
                .with_prime_extra(LOW_LATENCY_PRIME_EXTRA)
        } else {
            FillController::new(TARGET_FILL, SLACK, WINDOW_BLOCKS)
        };
    }

    fn fill(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        self.stats.out_chunk.store(frames as u32, Ordering::Relaxed);
        if frames == 0 || frames > MAX_FRAMES {
            out.fill(0.0);
            return;
        }
        let proc = &mut self.proc;
        self.commands.drain(|cmd| proc.apply_pad_command(cmd));
        match self.reader.read(frames) {
            Read::Priming => {
                out.fill(0.0);
                self.stats.out_peak.set(0.0);
            }
            Read::Underrun => {
                out.fill(0.0);
                self.stats.out_peak.set(0.0);
                self.stats.underruns.fetch_add(1, Ordering::Relaxed);
            }
            Read::Block => {
                self.controls.settings.snapshot(&mut self.settings);
                let monitoring = self.mon_active.load(Ordering::Acquire);
                let voice_only = monitoring && !self.controls.monitor_soundboard.load(Ordering::Relaxed);
                let block = self.reader.block(frames);
                self.proc.process_voice(block, &self.settings);
                if voice_only {
                    self.mon_scratch[..frames * 2].copy_from_slice(block);
                }
                self.proc.finish(block, &self.settings);
                if monitoring {
                    if !voice_only {
                        self.mon_scratch[..frames * 2].copy_from_slice(block);
                    }
                    let target = db_to_lin(self.controls.monitor_gain_db.get());
                    let step = (target - self.mon_gain_prev) / frames as f32;
                    let mut g = self.mon_gain_prev;
                    for fr in self.mon_scratch[..frames * 2].as_chunks_mut::<2>().0 {
                        g += step;
                        fr[0] *= g;
                        fr[1] *= g;
                    }
                    self.mon_gain_prev = target;
                    if self.mon_prod.vacant_len() >= frames * 2 {
                        self.mon_prod.push_slice(&self.mon_scratch[..frames * 2]);
                    }
                }
                let mut peak = 0f32;
                for (o, s) in out.iter_mut().zip(block.iter()) {
                    let v = s.clamp(-1.0, 1.0);
                    peak = peak.max(v.abs());
                    *o = v;
                }
                self.stats.out_peak.set(peak);
                let fill = self.reader.fill_frames() as f32;
                self.fill_avg += (fill - self.fill_avg) * 0.02;
                self.stats.fill_frames.store(self.fill_avg as u32, Ordering::Relaxed);
            }
        }
    }
}

impl lowlat::Render for OutputChain {
    fn render(&mut self, out: &mut [f32]) {
        self.fill(out);
    }
}

pub fn start(
    input: &Device,
    input_name: &str,
    output: &Device,
    output_name: &str,
    controls: Arc<Controls>,
    commands: Arc<CommandQueue>,
    low_latency: bool,
) -> Result<Engine, String> {
    let in_cfg = input
        .default_input_config()
        .map_err(|e| format!("Micro « {input_name} » inutilisable : {}", describe(&e)))?;
    validate_format(input_name, in_cfg.sample_rate(), in_cfg.sample_format())?;
    let out_cfg = output
        .default_output_config()
        .map_err(|e| format!("Sortie « {output_name} » inutilisable : {}", describe(&e)))?;
    validate_format(output_name, out_cfg.sample_rate(), cpal::SampleFormat::F32)?;

    let in_channels = in_cfg.channels() as usize;
    let in_stream_cfg = StreamConfig {
        channels: in_cfg.channels(),
        sample_rate: in_cfg.sample_rate(),
        buffer_size: BufferSize::Default,
    };
    let out_stream_cfg = StreamConfig {
        channels: 2,
        sample_rate: crate::devices::REQUIRED_RATE,
        buffer_size: BufferSize::Default,
    };

    let stats = Arc::new(Stats::default());
    let (tx, rx) = mpsc::channel();
    let (mut prod, cons) = HeapRb::<f32>::new(RING_FRAMES * 2).split();
    let monitor_rb = Arc::new(HeapRb::<f32>::new(RING_FRAMES * 2));
    let monitor_active = Arc::new(AtomicBool::new(false));

    // Capture : convertit en stereo entrelace et pousse des trames entieres dans la file.
    let in_stats = stats.clone();
    let mut in_scratch = vec![0f32; MAX_FRAMES * 2];
    let tx_in = tx.clone();
    let input_stream = input
        .build_input_stream::<f32, _, _>(
            in_stream_cfg,
            move |data: &[f32], _| {
                in_stats
                    .in_chunk
                    .store((data.len() / in_channels) as u32, Ordering::Relaxed);
                let mut peak = 0f32;
                for block in data.chunks(MAX_FRAMES * in_channels) {
                    let frames = block.len() / in_channels;
                    for (i, fr) in block.chunks_exact(in_channels).enumerate() {
                        let l = fr[0];
                        let r = if in_channels > 1 { fr[1] } else { fr[0] };
                        in_scratch[2 * i] = l;
                        in_scratch[2 * i + 1] = r;
                        peak = peak.max(l.abs()).max(r.abs());
                    }
                    if prod.vacant_len() >= 2 * frames {
                        prod.push_slice(&in_scratch[..2 * frames]);
                    } else {
                        in_stats.overruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
                in_stats.in_peak.set(peak);
            },
            move |e| {
                if !is_fatal(&e) {
                    return;
                }
                let _ = tx_in.send(EngineEvent::StreamFailed {
                    role: Role::Input,
                    device_lost: e.kind() == ErrorKind::DeviceNotAvailable,
                    message: describe(&e),
                });
            },
            None,
        )
        .map_err(|e| format!("Impossible d'ouvrir le micro « {input_name} » : {}", describe(&e)))?;

    // Sortie : lit la file, corrige la derive, applique la chaine, alimente le retour micro.
    let mut chain = OutputChain {
        reader: Reader::new(cons),
        proc: Processor::new(controls.meters.clone()),
        settings: Settings::default(),
        controls: controls.clone(),
        commands: commands.clone(),
        stats: stats.clone(),
        mon_prod: CachingProd::new(monitor_rb.clone()),
        mon_scratch: vec![0f32; MAX_FRAMES * 2],
        mon_active: monitor_active.clone(),
        mon_gain_prev: 1.0,
        fill_avg: 0.0,
    };
    let tx_out = tx.clone();

    let mut low_latency_note = None;
    let mut low_latency_period = None;
    let mut handle = None;
    if low_latency {
        match output.id() {
            Ok(id) => {
                chain.set_low_latency(true);
                let tx_low = tx.clone();
                let on_failure: lowlat::OnFailure = Box::new(move |device_lost, message| {
                    let _ = tx_low.send(EngineEvent::StreamFailed { role: Role::Output, device_lost, message });
                });
                match lowlat::open(id.id(), chain, on_failure) {
                    Ok(out) => {
                        low_latency_period = Some(out.period_frames);
                        handle = Some(OutputHandle::LowLatency(out));
                        // La chaine appartient desormais au flux ; le repli n'est plus possible.
                        chain = OutputChain::placeholder();
                    }
                    Err(e) => {
                        low_latency_note = Some(e.message);
                        match e.render {
                            Some(c) => {
                                chain = c;
                                chain.set_low_latency(false);
                            }
                            None => return Err("Impossible d'ouvrir la sortie en faible latence.".into()),
                        }
                    }
                }
            }
            Err(e) => low_latency_note = Some(describe(&e)),
        }
    }
    let output_handle = match handle {
        Some(h) => h,
        None => {
            let stream = output
                .build_output_stream::<f32, _, _>(
                    out_stream_cfg,
                    move |out: &mut [f32], _| chain.fill(out),
                    move |e| {
                        if !is_fatal(&e) {
                            return;
                        }
                        let _ = tx_out.send(EngineEvent::StreamFailed {
                            role: Role::Output,
                            device_lost: e.kind() == ErrorKind::DeviceNotAvailable,
                            message: describe(&e),
                        });
                    },
                    None,
                )
                .map_err(|e| format!("Impossible d'ouvrir la sortie « {output_name} » : {}", describe(&e)))?;
            OutputHandle::Standard(stream)
        }
    };

    input_stream
        .play()
        .map_err(|e| format!("Démarrage du micro impossible : {}", describe(&e)))?;
    if let OutputHandle::Standard(stream) = &output_handle {
        stream
            .play()
            .map_err(|e| format!("Démarrage de la sortie impossible : {}", describe(&e)))?;
    }

    commands.set_active(true);
    Ok(Engine {
        _input: input_stream,
        _output: output_handle,
        low_latency_period,
        low_latency_note,
        monitor: None,
        monitor_rb,
        monitor_active,
        commands,
        tx,
        stats,
        events: rx,
    })
}

/// Diagnostic (`--probe-engine`) : micro par defaut vers « CABLE Input » pendant `seconds`, puis
/// affiche la latence estimee et les compteurs de coupures, pour chaque mode de sortie.
pub fn probe(seconds: u64) {
    use crate::devices;
    let host = cpal::default_host();
    let (inputs, default_in) = devices::list_inputs(&host);
    let (outputs, _) = devices::list_outputs(&host);
    let in_names: Vec<String> = inputs.iter().map(|e| e.name.clone()).collect();
    let out_names: Vec<String> = outputs.iter().map(|e| e.name.clone()).collect();
    let (Some(i), Some(o)) = (
        devices::pick_input(&in_names, None, default_in.map(|d| in_names[d].as_str())),
        devices::cable_input_index(&out_names),
    ) else {
        println!("micro ou CABLE Input introuvable");
        return;
    };
    for low in [true, false] {
        let controls = Arc::new(Controls::new(Settings::default(), -12.0, true));
        let queue = Arc::new(CommandQueue::new());
        let engine = match start(&inputs[i].device, &inputs[i].name, &outputs[o].device, &outputs[o].name, controls, queue, low) {
            Ok(e) => e,
            Err(e) => {
                println!("echec : {e}");
                continue;
            }
        };
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        let s = &engine.stats;
        println!(
            "faible latence demandee : {low} | periode : {:?} | note : {:?}
  micro {} | bloc entree {} | bloc sortie {} | remplissage moyen {} | latence estimee {:.1} ms | coupures {} | debordements {}",
            engine.low_latency_period,
            engine.low_latency_note,
            inputs[i].name,
            s.in_chunk.load(Ordering::Relaxed),
            s.out_chunk.load(Ordering::Relaxed),
            s.fill_frames.load(Ordering::Relaxed),
            engine.latency_ms(true),
            s.underruns.load(Ordering::Relaxed),
            s.overruns.load(Ordering::Relaxed),
        );
    }
}
