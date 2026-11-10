//! Egaliseur : filtre passe-haut d'ordre 2 puis cinq bandes (etagere basse, trois cloches,
//! etagere haute).

use crate::biquad::{Coeffs, SmoothBiquad};
use crate::settings::Settings;

/// Facteur de qualite du passe-haut (Butterworth).
const HPF_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub struct Equalizer {
    hpf: SmoothBiquad,
    bands: [SmoothBiquad; 5],
}

impl Equalizer {
    pub fn new() -> Self {
        Self { hpf: SmoothBiquad::new(), bands: std::array::from_fn(|_| SmoothBiquad::new()) }
    }

    /// Met a jour les coefficients cibles depuis les reglages (appele a chaque bloc).
    pub fn configure(&mut self, s: &Settings) {
        self.hpf.set_target(if s.hpf_on {
            Coeffs::high_pass(s.hpf_freq, HPF_Q)
        } else {
            Coeffs::IDENTITY
        });
        for (i, (filter, b)) in self.bands.iter_mut().zip(s.eq.iter()).enumerate() {
            let target = if !s.eq_on || b.gain_db.abs() < 0.01 {
                Coeffs::IDENTITY
            } else {
                match i {
                    0 => Coeffs::low_shelf(b.freq, b.gain_db, b.q),
                    4 => Coeffs::high_shelf(b.freq, b.gain_db, b.q),
                    _ => Coeffs::peaking(b.freq, b.gain_db, b.q),
                }
            };
            filter.set_target(target);
        }
    }

    pub fn process(&mut self, buf: &mut [f32], channels: usize) {
        self.hpf.process(buf, channels);
        for b in &mut self.bands {
            b.process(buf, channels);
        }
    }
}

impl Default for Equalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{max_step, peak_dbfs, sine};

    fn level_after(s: &Settings, freq: f32) -> f32 {
        let mut eq = Equalizer::new();
        eq.configure(s);
        let mut buf = sine(freq, -20.0, 24_000, 2);
        eq.process(&mut buf, 2);
        peak_dbfs(&buf[24_000..]) + 20.0
    }

    #[test]
    fn bypass_is_identical() {
        let mut eq = Equalizer::new();
        eq.configure(&Settings::default());
        let src = sine(1000.0, -10.0, 2400, 2);
        let mut buf = src.clone();
        eq.process(&mut buf, 2);
        assert_eq!(src, buf);
    }

    #[test]
    fn bell_boosts_center_only() {
        let mut s = Settings::default();
        s.eq_on = true;
        s.eq[2].freq = 1000.0;
        s.eq[2].q = 1.0;
        s.eq[2].gain_db = 6.0;
        let at_center = level_after(&s, 1000.0);
        let far = level_after(&s, 100.0);
        assert!((at_center - 6.0).abs() < 0.3, "centre {at_center} dB");
        assert!(far.abs() < 1.0, "100 Hz {far} dB");
    }

    #[test]
    fn high_pass_attenuates_low_frequencies() {
        let mut s = Settings::default();
        s.hpf_on = true;
        s.hpf_freq = 80.0;
        let low = level_after(&s, 40.0);
        let mid = level_after(&s, 1000.0);
        assert!(mid - low >= 10.0, "40 Hz {low} dB, 1 kHz {mid} dB");
    }

    #[test]
    fn shelves_act_on_their_side() {
        let mut s = Settings::default();
        s.eq_on = true;
        s.eq[0].gain_db = 6.0;
        s.eq[4].gain_db = -6.0;
        assert!(level_after(&s, 30.0) > 4.5);
        assert!(level_after(&s, 15_000.0) < -4.5);
        assert!(level_after(&s, 1000.0).abs() < 1.0);
    }

    #[test]
    fn live_gain_change_has_no_click() {
        let mut s = Settings::default();
        s.eq_on = true;
        s.eq[2].gain_db = 0.0;
        let mut eq = Equalizer::new();
        eq.configure(&s);
        let mut out = Vec::new();
        let mut buf = sine(1000.0, -20.0, 4800, 1);
        // duplique en stereo
        let mut st: Vec<f32> = buf.iter().flat_map(|v| [*v, *v]).collect();
        eq.process(&mut st, 2);
        s.eq[2].gain_db = 12.0;
        eq.configure(&s);
        buf = sine(1000.0, -20.0, 4800, 1);
        let mut st2: Vec<f32> = buf.iter().flat_map(|v| [*v, *v]).collect();
        eq.process(&mut st2, 2);
        out.extend(st.iter().step_by(2));
        out.extend(st2.iter().step_by(2));
        assert!(max_step(&out) < 0.1, "saut {}", max_step(&out));
    }
}
