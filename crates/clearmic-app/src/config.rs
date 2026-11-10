//! Configuration persistante : `%APPDATA%\ClearMic\config.json`, ecriture atomique.

use std::path::{Path, PathBuf};

use clearmic_dsp::settings::Settings;
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PadConfig {
    pub path: String,
    pub name: String,
    pub gain_db: f32,
    /// Raccourci clavier global, par exemple « Ctrl+Alt+1 ».
    pub hotkey: Option<String>,
}

impl Default for PadConfig {
    fn default() -> Self {
        Self { path: String::new(), name: String::new(), gain_db: 0.0, hotkey: None }
    }
}

/// Reglages de voix enregistres sous un nom.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoicePreset {
    pub name: String,
    pub settings: Settings,
}

pub const MAX_PRESETS: usize = 64;

/// Le retour micro et l'etat "en marche" ne sont volontairement jamais restaures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub monitor_device: Option<String>,
    pub monitor_gain_db: f32,
    pub monitor_soundboard: bool,
    pub settings: Settings,
    pub pads: Vec<PadConfig>,
    /// Demarre le traitement tout seul au lancement de l'application.
    pub autostart_engine: bool,
    /// Sortie WASAPI a periode minimale (repli automatique sur le mode standard).
    pub low_latency: bool,
    /// Raccourci global « tout arreter » du soundboard.
    pub stop_all_hotkey: Option<String>,
    pub presets: Vec<VoicePreset>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: VERSION,
            input_device: None,
            output_device: None,
            monitor_device: None,
            monitor_gain_db: -12.0,
            monitor_soundboard: true,
            settings: Settings::default(),
            pads: Vec::new(),
            autostart_engine: false,
            low_latency: true,
            stop_all_hotkey: None,
            presets: Vec::new(),
        }
    }
}

impl Config {
    pub fn sanitized(mut self) -> Self {
        self.settings = self.settings.sanitized();
        self.monitor_gain_db = if self.monitor_gain_db.is_nan() {
            -12.0
        } else {
            self.monitor_gain_db.clamp(-60.0, 0.0)
        };
        for p in &mut self.pads {
            p.gain_db = if p.gain_db.is_nan() { 0.0 } else { p.gain_db.clamp(-60.0, 6.0) };
        }
        self.pads.truncate(clearmic_dsp::player::MAX_PADS);
        self.presets.truncate(MAX_PRESETS);
        for p in &mut self.presets {
            p.settings = p.settings.sanitized();
        }
        self
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("ClearMic").join("config.json")
}

pub struct Loaded {
    pub config: Config,
    /// Message a afficher (fichier corrompu ...).
    pub notice: Option<String>,
}

/// Lecture tolerante : fichier absent = valeurs par defaut ; illisible = renomme en `.bak`.
pub fn load(path: &Path) -> Loaded {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Loaded { config: Config::default(), notice: None };
    };
    match serde_json::from_str::<Config>(&text) {
        Ok(c) => Loaded { config: c.sanitized(), notice: None },
        Err(_) => {
            let bak = path.with_extension("json.bak");
            let _ = std::fs::remove_file(&bak);
            let moved = std::fs::rename(path, &bak).is_ok();
            let notice = if moved {
                format!(
                    "Configuration illisible : valeurs par défaut utilisées, ancien fichier conservé dans {}.",
                    bak.display()
                )
            } else {
                "Configuration illisible : valeurs par défaut utilisées.".to_string()
            };
            Loaded { config: Config::default(), notice: Some(notice) }
        }
    }
}

/// Ecriture atomique : fichier temporaire puis renommage.
pub fn save(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clearmic-cfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip() {
        let dir = temp_dir("rt");
        let path = dir.join("config.json");
        let mut c = Config::default();
        c.input_device = Some("Micro USB".into());
        c.settings.boost_db = 9.0;
        c.settings.eq[3].gain_db = -3.0;
        c.pads.push(PadConfig {
            path: "C:/a.wav".into(),
            name: "a".into(),
            gain_db: -6.0,
            hotkey: Some("Ctrl+Alt+1".into()),
        });
        c.autostart_engine = true;
        c.low_latency = false;
        c.stop_all_hotkey = Some("Ctrl+Alt+0".into());
        c.presets.push(VoicePreset { name: "Stream".into(), settings: Settings { boost_db: 6.0, ..Settings::default() } });
        save(&path, &c).unwrap();
        let back = load(&path);
        assert!(back.notice.is_none());
        assert_eq!(back.config, c);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        let l = load(&dir.join("config.json"));
        assert_eq!(l.config, Config::default());
        assert!(l.notice.is_none());
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = temp_dir("corrupt");
        let path = dir.join("config.json");
        std::fs::write(&path, "{ pas du json").unwrap();
        let l = load(&path);
        assert_eq!(l.config, Config::default());
        assert!(l.notice.is_some());
        assert!(!path.exists());
        assert!(dir.join("config.json.bak").exists());
    }

    #[test]
    fn save_leaves_no_temp_file_and_replaces_atomically() {
        let dir = temp_dir("atomic");
        let path = dir.join("config.json");
        save(&path, &Config::default()).unwrap();
        let mut c = Config::default();
        c.monitor_gain_db = -30.0;
        save(&path, &c).unwrap();
        assert!(!dir.join("config.json.tmp").exists());
        assert_eq!(load(&path).config.monitor_gain_db, -30.0);
    }

    #[test]
    fn interrupted_write_keeps_old_config() {
        let dir = temp_dir("interrupted");
        let path = dir.join("config.json");
        let mut c = Config::default();
        c.settings.boost_db = 3.0;
        save(&path, &c).unwrap();
        // Ecriture interrompue : seul le fichier temporaire est abime, jamais le fichier principal.
        std::fs::write(dir.join("config.json.tmp"), "{ tronque").unwrap();
        assert_eq!(load(&path).config.settings.boost_db, 3.0);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let dir = temp_dir("clamp");
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"monitor_gain_db": 40.0, "settings": {"boost_db": 500.0}}"#).unwrap();
        let c = load(&path).config;
        assert_eq!(c.monitor_gain_db, 0.0);
        assert_eq!(c.settings.boost_db, 30.0);
    }
}
