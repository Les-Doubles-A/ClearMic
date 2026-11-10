//! Raccourcis clavier globaux (RegisterHotKey) pour declencher les pads du soundboard, y compris
//! fenetre reduite ou jeu au premier plan. Un thread dedie porte la boucle de messages et envoie
//! les commandes directement au moteur, sans passer par l'interface.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use clearmic_dsp::player::PadCommand;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_APP, WM_HOTKEY,
};

use crate::audio::CommandQueue;
use crate::sound::Bank;

const ERROR_HOTKEY_ALREADY_REGISTERED: i32 = 1409;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u32,
}

/// Nom d'une touche vers son code virtuel Windows. Les noms sont ceux d'`egui::Key::name`.
pub fn vk_for(name: &str) -> Option<u32> {
    let b = name.as_bytes();
    match name {
        _ if b.len() == 1 && b[0].is_ascii_uppercase() => Some(b[0] as u32),
        _ if b.len() == 1 && b[0].is_ascii_digit() => Some(b[0] as u32),
        "Space" => Some(0x20),
        "Enter" => Some(0x0D),
        "Insert" => Some(0x2D),
        "Delete" => Some(0x2E),
        "Home" => Some(0x24),
        "End" => Some(0x23),
        "PageUp" => Some(0x21),
        "PageDown" => Some(0x22),
        "Left" => Some(0x25),
        "Up" => Some(0x26),
        "Right" => Some(0x27),
        "Down" => Some(0x28),
        _ => {
            let n: u32 = name.strip_prefix('F')?.parse().ok()?;
            (1..=24).contains(&n).then(|| 0x70 + n - 1)
        }
    }
}

fn name_for(vk: u32) -> Option<String> {
    match vk {
        0x41..=0x5A | 0x30..=0x39 => Some((vk as u8 as char).to_string()),
        0x70..=0x87 => Some(format!("F{}", vk - 0x70 + 1)),
        0x20 => Some("Space".into()),
        0x0D => Some("Enter".into()),
        0x2D => Some("Insert".into()),
        0x2E => Some("Delete".into()),
        0x24 => Some("Home".into()),
        0x23 => Some("End".into()),
        0x21 => Some("PageUp".into()),
        0x22 => Some("PageDown".into()),
        0x25 => Some("Left".into()),
        0x26 => Some("Up".into()),
        0x27 => Some("Right".into()),
        0x28 => Some("Down".into()),
        _ => None,
    }
}

impl Combo {
    /// Construit une combinaison depuis le nom de la touche et les modificateurs. Une touche seule
    /// n'est acceptee que pour les touches de fonction (sinon elle volerait la frappe normale).
    pub fn new(key_name: &str, ctrl: bool, alt: bool, shift: bool, win: bool) -> Result<Self, String> {
        let vk = vk_for(key_name).ok_or_else(|| format!("touche « {key_name} » non prise en charge"))?;
        let is_function = (0x70..=0x87).contains(&vk);
        if !(ctrl || alt || shift || win || is_function) {
            return Err("ajoutez Ctrl, Alt, Maj ou Windows (ou utilisez une touche F1 à F24)".into());
        }
        Ok(Self { ctrl, alt, shift, win, vk })
    }

    /// Relit « Ctrl+Alt+F5 » (ordre libre).
    pub fn parse(text: &str) -> Result<Self, String> {
        let (mut ctrl, mut alt, mut shift, mut win) = (false, false, false, false);
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            match part {
                "Ctrl" => ctrl = true,
                "Alt" => alt = true,
                "Maj" | "Shift" => shift = true,
                "Win" => win = true,
                "" => return Err("raccourci vide".into()),
                k if key.is_none() => key = Some(k),
                _ => return Err("une seule touche attendue".into()),
            }
        }
        Self::new(key.ok_or("touche manquante")?, ctrl, alt, shift, win)
    }

    fn modifiers(self) -> HOT_KEY_MODIFIERS {
        let mut m = MOD_NOREPEAT;
        if self.ctrl {
            m |= MOD_CONTROL;
        }
        if self.alt {
            m |= MOD_ALT;
        }
        if self.shift {
            m |= MOD_SHIFT;
        }
        if self.win {
            m |= MOD_WIN;
        }
        m
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Maj"), (self.win, "Win")] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", name_for(self.vk).unwrap_or_else(|| format!("0x{:X}", self.vk)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pad(usize),
    StopAll,
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub action: Action,
    pub combo: Combo,
}

enum Request {
    /// Remplace tous les raccourcis ; la reponse liste les echecs (indice dans la liste, raison).
    Set(Vec<Binding>, Sender<Vec<(usize, String)>>),
    Quit,
}

pub struct Hotkeys {
    thread_id: u32,
    requests: Sender<Request>,
    join: Option<JoinHandle<()>>,
}

fn wake(thread_id: u32) {
    unsafe {
        let _ = PostThreadMessageW(thread_id, WM_APP, Default::default(), Default::default());
    }
}

impl Hotkeys {
    pub fn spawn(queue: Arc<CommandQueue>, bank: Arc<Bank>) -> Result<Self, String> {
        let (req_tx, req_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let join = std::thread::Builder::new()
            .name("clearmic-hotkeys".into())
            .spawn(move || run(req_rx, ready_tx, queue, bank))
            .map_err(|e| format!("thread des raccourcis : {e}"))?;
        let thread_id = ready_rx.recv().map_err(|_| "thread des raccourcis interrompu".to_string())?;
        Ok(Self { thread_id, requests: req_tx, join: Some(join) })
    }

    /// Enregistre exactement `bindings` (les precedents sont retires). Renvoie les echecs.
    pub fn set(&self, bindings: Vec<Binding>) -> Vec<(usize, String)> {
        let (tx, rx) = mpsc::channel();
        if self.requests.send(Request::Set(bindings, tx)).is_err() {
            return vec![(0, "thread des raccourcis arrêté".into())];
        }
        wake(self.thread_id);
        rx.recv().unwrap_or_default()
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Quit);
        wake(self.thread_id);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run(requests: Receiver<Request>, ready: Sender<u32>, queue: Arc<CommandQueue>, bank: Arc<Bank>) {
    unsafe {
        // Cree la file de messages du thread avant d'annoncer son identifiant.
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, WM_APP, WM_APP, PM_NOREMOVE);
        let _ = ready.send(GetCurrentThreadId());
        let mut registered: HashMap<i32, Action> = HashMap::new();
        loop {
            if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                break;
            }
            if msg.message == WM_HOTKEY {
                match registered.get(&(msg.wParam.0 as i32)) {
                    Some(Action::Pad(pad)) => {
                        if let Some(cmd) = bank.play_command(*pad) {
                            queue.push(cmd);
                        }
                    }
                    Some(Action::StopAll) => {
                        queue.push(PadCommand::StopAll);
                    }
                    None => {}
                }
            } else if msg.message == WM_APP {
                while let Ok(req) = requests.try_recv() {
                    match req {
                        Request::Quit => {
                            for id in registered.keys() {
                                let _ = UnregisterHotKey(None, *id);
                            }
                            return;
                        }
                        Request::Set(bindings, reply) => {
                            for id in registered.keys() {
                                let _ = UnregisterHotKey(None, *id);
                            }
                            registered.clear();
                            let mut failures = Vec::new();
                            for (i, b) in bindings.iter().enumerate() {
                                let id = i as i32 + 1;
                                match RegisterHotKey(None, id, b.combo.modifiers(), b.combo.vk) {
                                    Ok(()) => {
                                        registered.insert(id, b.action);
                                    }
                                    Err(e) => {
                                        let reason = if (e.code().0 & 0xFFFF) == ERROR_HOTKEY_ALREADY_REGISTERED {
                                            "déjà utilisé par une autre application".to_string()
                                        } else {
                                            e.to_string()
                                        };
                                        failures.push((i, reason));
                                    }
                                }
                            }
                            let _ = reply.send(failures);
                        }
                    }
                }
            }
        }
        for id in registered.keys() {
            let _ = UnregisterHotKey(None, *id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_roundtrip() {
        let c = Combo::new("F5", true, true, false, false).unwrap();
        assert_eq!(c.to_string(), "Ctrl+Alt+F5");
        assert_eq!(Combo::parse("Ctrl+Alt+F5").unwrap(), c);
        let d = Combo::new("A", true, false, true, false).unwrap();
        assert_eq!(d.to_string(), "Ctrl+Maj+A");
        assert_eq!(Combo::parse("Maj+Ctrl+A").unwrap(), d);
    }

    #[test]
    fn bare_letters_are_refused_but_function_keys_are_not() {
        assert!(Combo::new("A", false, false, false, false).is_err());
        assert!(Combo::new("5", false, false, false, false).is_err());
        assert!(Combo::new("F9", false, false, false, false).is_ok());
    }

    #[test]
    fn unknown_keys_and_garbage_are_rejected() {
        assert!(Combo::parse("Ctrl+").is_err());
        assert!(Combo::parse("Ctrl+Escape").is_err());
        assert!(Combo::parse("").is_err());
        assert!(Combo::parse("Ctrl+A+B").is_err());
        assert!(Combo::parse("Ctrl+F25").is_err());
    }

    #[test]
    fn key_codes_match_windows() {
        assert_eq!(vk_for("A"), Some(0x41));
        assert_eq!(vk_for("0"), Some(0x30));
        assert_eq!(vk_for("F1"), Some(0x70));
        assert_eq!(vk_for("F24"), Some(0x87));
        assert_eq!(vk_for("PageDown"), Some(0x22));
    }

    /// Enregistre pour de vrai une combinaison rare, verifie le conflit puis libere tout.
    #[test]
    fn registration_detects_conflicts() {
        let hk = Hotkeys::spawn(Arc::new(CommandQueue::new()), Arc::new(Bank::default())).unwrap();
        let combo = Combo::new("F24", true, true, true, false).unwrap();
        let ok = hk.set(vec![Binding { action: Action::StopAll, combo }]);
        assert!(ok.is_empty(), "{ok:?}");
        // Meme combinaison deux fois dans la liste : la seconde est refusee.
        let dup = hk.set(vec![
            Binding { action: Action::StopAll, combo },
            Binding { action: Action::Pad(1), combo },
        ]);
        assert_eq!(dup.len(), 1);
        assert_eq!(dup[0].0, 1);
        assert!(hk.set(vec![]).is_empty());
    }
}
