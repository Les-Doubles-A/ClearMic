//! Utilitaires de mesure et de generation pour les tests (sinus, bruit, niveaux).

use crate::SAMPLE_RATE;
use crate::param::{db_to_lin, lin_to_db};

/// Sinus de crete `peak_dbfs`, `frames` trames, `channels` canaux identiques entrelaces.
pub fn sine(freq: f32, peak_dbfs: f32, frames: usize, channels: usize) -> Vec<f32> {
    let amp = db_to_lin(peak_dbfs);
    let mut out = Vec::with_capacity(frames * channels);
    for n in 0..frames {
        let v = amp * (2.0 * std::f32::consts::PI * freq * n as f32 / SAMPLE_RATE).sin();
        for _ in 0..channels {
            out.push(v);
        }
    }
    out
}

/// Bruit blanc deterministe de niveau RMS `rms_dbfs` (generateur congruentiel, graine fixe).
pub fn white_noise(rms_dbfs: f32, samples: usize) -> Vec<f32> {
    let mut state: u32 = 0x1234_5678;
    let mut raw: Vec<f32> = (0..samples)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        })
        .collect();
    let current = rms_of(&raw);
    let scale = db_to_lin(rms_dbfs) / current.max(1e-12);
    raw.iter_mut().for_each(|s| *s *= scale);
    raw
}

fn rms_of(buf: &[f32]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
}

/// Niveau RMS en dBFS.
pub fn rms_dbfs(buf: &[f32]) -> f32 {
    lin_to_db(rms_of(buf))
}

/// Niveau crete en dBFS.
pub fn peak_dbfs(buf: &[f32]) -> f32 {
    lin_to_db(buf.iter().fold(0.0f32, |m, s| m.max(s.abs())))
}

/// Plus grand ecart absolu entre deux echantillons consecutifs (detection de clics).
pub fn max_step(buf: &[f32]) -> f32 {
    buf.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_peak_level() {
        let s = sine(1000.0, -20.0, 4800, 1);
        assert!((peak_dbfs(&s) + 20.0).abs() < 0.05);
    }

    #[test]
    fn sine_rms_is_peak_minus_3db() {
        let s = sine(1000.0, -10.0, 4800, 1);
        assert!((rms_dbfs(&s) + 13.01).abs() < 0.05);
    }

    #[test]
    fn noise_rms_level() {
        let n = white_noise(-40.0, 48_000);
        assert!((rms_dbfs(&n) + 40.0).abs() < 0.05);
    }

    #[test]
    fn max_step_detects_jump() {
        assert!((max_step(&[0.0, 0.1, 0.9, 0.9]) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn stereo_sine_is_interleaved() {
        let s = sine(1000.0, -6.0, 100, 2);
        assert_eq!(s.len(), 200);
        assert_eq!(s[10], s[11]);
    }
}
