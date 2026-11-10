//! Compresseur a detection de crete, genou doux, attaque et relachement reglables.

use crate::SAMPLE_RATE;
use crate::param::{db_to_lin, lin_to_db};
use crate::settings::CompressorSettings;

/// Decroissance du detecteur de crete (instantane a la montee), en secondes.
const DETECTOR_DECAY_S: f32 = 0.010;

pub struct Compressor {
    p: CompressorSettings,
    env: f32,
    /// Reduction de gain courante en dB (positive).
    gr_db: f32,
    decay: f32,
    attack: f32,
    release: f32,
    makeup: f32,
}

fn coeff(time_s: f32) -> f32 {
    1.0 - (-1.0 / (time_s.max(1e-4) * SAMPLE_RATE)).exp()
}

impl Compressor {
    pub fn new() -> Self {
        let p = CompressorSettings::default();
        let mut c = Self {
            p,
            env: 0.0,
            gr_db: 0.0,
            decay: (-1.0 / (DETECTOR_DECAY_S * SAMPLE_RATE)).exp(),
            attack: 0.0,
            release: 0.0,
            makeup: 1.0,
        };
        c.configure(&p);
        c
    }

    pub fn configure(&mut self, p: &CompressorSettings) {
        self.p = *p;
        self.attack = coeff(p.attack_ms / 1000.0);
        self.release = coeff(p.release_ms / 1000.0);
        self.makeup = db_to_lin(p.makeup_db);
    }

    pub fn gain_reduction_db(&self) -> f32 {
        self.gr_db
    }

    /// Reduction de gain statique (dB, positive) pour un niveau d'entree en dB.
    fn static_reduction(&self, level_db: f32) -> f32 {
        let (t, r, w) = (self.p.threshold_db, self.p.ratio, self.p.knee_db);
        let over = level_db - t;
        let slope = 1.0 - 1.0 / r;
        if w > 0.0 && over.abs() <= w / 2.0 {
            let x = over + w / 2.0;
            slope * x * x / (2.0 * w)
        } else if over > 0.0 {
            slope * over
        } else {
            0.0
        }
    }

    /// Bloc entrelace ; la detection est liee entre les canaux.
    pub fn process(&mut self, buf: &mut [f32], channels: usize) {
        for frame in buf.chunks_exact_mut(channels.max(1)) {
            let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            self.env = peak.max(self.env * self.decay);
            let target = self.static_reduction(lin_to_db(self.env));
            let k = if target > self.gr_db { self.attack } else { self.release };
            self.gr_db += (target - self.gr_db) * k;
            let g = db_to_lin(-self.gr_db) * self.makeup;
            for s in frame {
                *s *= g;
            }
        }
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{peak_dbfs, sine};

    fn base() -> CompressorSettings {
        CompressorSettings {
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            knee_db: 0.0,
            makeup_db: 0.0,
        }
    }

    fn out_peak(p: CompressorSettings, input_dbfs: f32) -> f32 {
        let mut c = Compressor::new();
        c.configure(&p);
        let mut buf = sine(1000.0, input_dbfs, 48_000, 1);
        c.process(&mut buf, 1);
        peak_dbfs(&buf[40_000..])
    }

    #[test]
    fn steady_state_ratio() {
        let out = out_peak(base(), -8.0);
        assert!((out + 17.0).abs() < 0.5, "sortie {out} dBFS");
    }

    #[test]
    fn below_threshold_is_unchanged() {
        let out = out_peak(base(), -40.0);
        assert!((out + 40.0).abs() < 0.05, "sortie {out} dBFS");
    }

    #[test]
    fn makeup_adds_gain() {
        let a = out_peak(base(), -8.0);
        let mut p = base();
        p.makeup_db = 6.0;
        let b = out_peak(p, -8.0);
        assert!((b - a - 6.0).abs() < 0.1, "{a} -> {b}");
    }

    #[test]
    fn attack_reaches_63_percent_at_attack_time() {
        let mut c = Compressor::new();
        c.configure(&base());
        let mut quiet = sine(1000.0, -40.0, 4800, 1);
        c.process(&mut quiet, 1);
        let loud = sine(1000.0, -8.0, 48_000, 1);
        let mut gr = Vec::new();
        for s in loud {
            let mut one = [s];
            c.process(&mut one, 1);
            gr.push(c.gain_reduction_db());
        }
        let final_gr = *gr.last().unwrap();
        let idx = gr.iter().position(|g| *g >= 0.632 * final_gr).unwrap();
        let ms = idx as f32 / SAMPLE_RATE * 1000.0;
        assert!((ms - 10.0).abs() <= 2.0, "63 % atteint apres {ms} ms (reduction finale {final_gr} dB)");
    }

    #[test]
    fn gain_reduction_returns_to_zero() {
        let mut c = Compressor::new();
        c.configure(&base());
        let mut loud = sine(1000.0, -8.0, 24_000, 1);
        c.process(&mut loud, 1);
        assert!(c.gain_reduction_db() > 5.0);
        let mut quiet = sine(1000.0, -50.0, 96_000, 1);
        c.process(&mut quiet, 1);
        assert!(c.gain_reduction_db() < 0.1);
    }

    #[test]
    fn soft_knee_is_continuous() {
        let mut p = base();
        p.knee_db = 10.0;
        let mut c = Compressor::new();
        c.configure(&p);
        let below = c.static_reduction(-25.1);
        let above = c.static_reduction(-14.9);
        assert!(below == 0.0);
        assert!((above - 0.75 * 5.1).abs() < 0.2);
        let mid = c.static_reduction(-20.0);
        assert!(mid > 0.0 && mid < 0.75 * 5.0);
    }
}
