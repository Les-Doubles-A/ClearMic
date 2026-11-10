//! Reglages de la chaine de traitement : valeurs simples, copiables, serialisables.
//!
//! L'interface modifie un `Settings` ; le rappel audio en lit un instantane au debut de chaque bloc
//! via `SettingsHandle::snapshot` (jamais bloquant : en cas de contention, l'ancien instantane
//! est conserve).

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::param::AtomicF32;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BandSettings {
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl BandSettings {
    const fn new(freq: f32, q: f32) -> Self {
        Self { freq, gain_db: 0.0, q }
    }
}

impl Default for BandSettings {
    fn default() -> Self {
        Self::new(1000.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressorSettings {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
    pub makeup_db: f32,
}

impl Default for CompressorSettings {
    fn default() -> Self {
        Self { threshold_db: -20.0, ratio: 4.0, attack_ms: 10.0, release_ms: 100.0, knee_db: 6.0, makeup_db: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub input_gain_db: f32,
    pub denoise_on: bool,
    /// Intensite du debruitage, 0.0 a 1.0.
    pub denoise_amount: f32,
    pub hpf_on: bool,
    pub hpf_freq: f32,
    pub eq_on: bool,
    /// Etagere basse, trois cloches, etagere haute.
    pub eq: [BandSettings; 5],
    pub comp_on: bool,
    pub comp: CompressorSettings,
    pub mono: bool,
    /// -100 (tout a gauche) a +100 (tout a droite).
    pub balance: f32,
    /// 0 (mono) a 200 (largeur doublee), 100 = inchange.
    pub width: f32,
    pub boost_db: f32,
    pub limiter_on: bool,
    pub soundboard_db: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_gain_db: 0.0,
            denoise_on: true,
            denoise_amount: 1.0,
            hpf_on: false,
            hpf_freq: 80.0,
            eq_on: false,
            eq: [
                BandSettings::new(100.0, 0.7),
                BandSettings::new(300.0, 1.0),
                BandSettings::new(1000.0, 1.0),
                BandSettings::new(3000.0, 1.0),
                BandSettings::new(8000.0, 0.7),
            ],
            comp_on: false,
            comp: CompressorSettings::default(),
            mono: false,
            balance: 0.0,
            width: 100.0,
            boost_db: 0.0,
            limiter_on: true,
            soundboard_db: 0.0,
        }
    }
}

fn clamp(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_nan() { fallback } else { v.clamp(lo, hi) }
}

impl Settings {
    /// Ramene chaque valeur dans ses bornes (fichier de config edite a la main, NaN).
    pub fn sanitized(mut self) -> Self {
        let d = Self::default();
        self.input_gain_db = clamp(self.input_gain_db, -20.0, 20.0, d.input_gain_db);
        self.denoise_amount = clamp(self.denoise_amount, 0.0, 1.0, d.denoise_amount);
        self.hpf_freq = clamp(self.hpf_freq, 20.0, 20_000.0, d.hpf_freq);
        for (b, db) in self.eq.iter_mut().zip(d.eq.iter()) {
            b.freq = clamp(b.freq, 20.0, 20_000.0, db.freq);
            b.gain_db = clamp(b.gain_db, -12.0, 12.0, 0.0);
            b.q = clamp(b.q, 0.1, 10.0, db.q);
        }
        let c = &mut self.comp;
        let dc = CompressorSettings::default();
        c.threshold_db = clamp(c.threshold_db, -60.0, 0.0, dc.threshold_db);
        c.ratio = clamp(c.ratio, 1.0, 20.0, dc.ratio);
        c.attack_ms = clamp(c.attack_ms, 1.0, 100.0, dc.attack_ms);
        c.release_ms = clamp(c.release_ms, 10.0, 1000.0, dc.release_ms);
        c.knee_db = clamp(c.knee_db, 0.0, 12.0, dc.knee_db);
        c.makeup_db = clamp(c.makeup_db, 0.0, 24.0, dc.makeup_db);
        self.balance = clamp(self.balance, -100.0, 100.0, 0.0);
        self.width = clamp(self.width, 0.0, 200.0, 100.0);
        self.boost_db = clamp(self.boost_db, 0.0, 30.0, 0.0);
        self.soundboard_db = clamp(self.soundboard_db, -60.0, 6.0, 0.0);
        self
    }
}

/// Reglages partages entre l'interface (ecriture) et le rappel audio (lecture non bloquante).
#[derive(Default)]
pub struct SettingsHandle(Mutex<Settings>);

impl SettingsHandle {
    pub fn new(s: Settings) -> Self {
        Self(Mutex::new(s))
    }

    /// Cote interface.
    pub fn set(&self, s: Settings) {
        if let Ok(mut g) = self.0.lock() {
            *g = s;
        }
    }

    /// Cote rappel audio : ne bloque jamais, conserve `cache` si le verrou est pris.
    pub fn snapshot(&self, cache: &mut Settings) {
        if let Ok(g) = self.0.try_lock() {
            *cache = *g;
        }
    }
}

/// Indicateurs ecrits par le rappel audio, lus par l'interface.
#[derive(Default)]
pub struct Meters {
    /// Reduction de gain du compresseur en dB (valeur positive).
    pub comp_gr_db: AtomicF32,
    /// Vrai tant que la sortie est tronquee a 0 dBFS (limiteur desactive).
    pub clip: AtomicBool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let mut s = Settings::default();
        s.eq[2].gain_db = 4.5;
        s.mono = true;
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"boost_db": 6.0}"#).unwrap();
        assert_eq!(s.boost_db, 6.0);
        assert!(s.limiter_on);
    }

    #[test]
    fn sanitize_clamps_and_replaces_nan() {
        let mut s = Settings::default();
        s.input_gain_db = 99.0;
        s.eq[0].freq = 5.0;
        s.eq[0].q = 50.0;
        s.eq[1].gain_db = f32::NAN;
        let s = s.sanitized();
        assert_eq!(s.input_gain_db, 20.0);
        assert_eq!(s.eq[0].freq, 20.0);
        assert_eq!(s.eq[0].q, 10.0);
        assert_eq!(s.eq[1].gain_db, 0.0);
    }

    #[test]
    fn snapshot_follows_set() {
        let h = SettingsHandle::default();
        let mut cache = Settings::default();
        let mut s = Settings::default();
        s.boost_db = 12.0;
        h.set(s);
        h.snapshot(&mut cache);
        assert_eq!(cache.boost_db, 12.0);
    }
}
