//! Filtres du second ordre (formules du "Audio EQ Cookbook" de R. Bristow-Johnson).

use std::f32::consts::PI;

use crate::SAMPLE_RATE;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Coeffs {
    pub const IDENTITY: Coeffs = Coeffs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn normalized(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    pub fn peaking(freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let (cos, alpha) = trig(freq, q);
        Self::normalized(
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        )
    }

    pub fn low_shelf(freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let (cos, alpha) = trig(freq, q);
        let t = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            a * ((a + 1.0) - (a - 1.0) * cos + t),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - t),
            (a + 1.0) + (a - 1.0) * cos + t,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - t,
        )
    }

    pub fn high_shelf(freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let (cos, alpha) = trig(freq, q);
        let t = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            a * ((a + 1.0) + (a - 1.0) * cos + t),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - t),
            (a + 1.0) - (a - 1.0) * cos + t,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - t,
        )
    }

    pub fn high_pass(freq: f32, q: f32) -> Self {
        let (cos, alpha) = trig(freq, q);
        Self::normalized(
            (1.0 + cos) / 2.0,
            -(1.0 + cos),
            (1.0 + cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

}

fn trig(freq: f32, q: f32) -> (f32, f32) {
    // Reste sous Nyquist pour garder des coefficients valides.
    let w0 = 2.0 * PI * freq.clamp(1.0, SAMPLE_RATE * 0.49) / SAMPLE_RATE;
    let (sin, cos) = w0.sin_cos();
    (cos, sin / (2.0 * q.max(0.05)))
}

/// Biquad stereo (forme transposee II) dont les coefficients glissent vers leur cible,
/// ce qui evite les clics quand un reglage bouge en direct.
pub struct SmoothBiquad {
    cur: Coeffs,
    target: Coeffs,
    z: [[f32; 2]; 2],
    k: f32,
}

/// Constante de temps du glissement des coefficients, en secondes.
const COEFF_SMOOTHING_S: f32 = 0.005;

impl SmoothBiquad {
    pub fn new() -> Self {
        Self {
            cur: Coeffs::IDENTITY,
            target: Coeffs::IDENTITY,
            z: [[0.0; 2]; 2],
            k: 1.0 - (-1.0 / (COEFF_SMOOTHING_S * SAMPLE_RATE)).exp(),
        }
    }

    pub fn set_target(&mut self, c: Coeffs) {
        self.target = c;
    }

    pub fn is_identity(&self) -> bool {
        self.cur == Coeffs::IDENTITY && self.target == Coeffs::IDENTITY
    }

    fn glide(&mut self) {
        let (c, t, k) = (&mut self.cur, &self.target, self.k);
        c.b0 += (t.b0 - c.b0) * k;
        c.b1 += (t.b1 - c.b1) * k;
        c.b2 += (t.b2 - c.b2) * k;
        c.a1 += (t.a1 - c.a1) * k;
        c.a2 += (t.a2 - c.a2) * k;
        let close = (t.b0 - c.b0).abs() + (t.b1 - c.b1).abs() + (t.b2 - c.b2).abs()
            + (t.a1 - c.a1).abs()
            + (t.a2 - c.a2).abs()
            < 1e-7;
        if close {
            self.cur = self.target;
        }
    }

    /// Traite un bloc entrelace de `channels` canaux (1 ou 2 utilises).
    pub fn process(&mut self, buf: &mut [f32], channels: usize) {
        if self.is_identity() {
            self.z = [[0.0; 2]; 2];
            return;
        }
        let channels = channels.clamp(1, 2);
        for frame in buf.chunks_exact_mut(channels) {
            if self.cur != self.target {
                self.glide();
            }
            let c = self.cur;
            for (ch, x) in frame.iter_mut().enumerate() {
                let z = &mut self.z[ch];
                let y = c.b0 * *x + z[0];
                z[0] = c.b1 * *x - c.a1 * y + z[1];
                z[1] = c.b2 * *x - c.a2 * y;
                *x = y;
            }
        }
    }
}

impl Default for SmoothBiquad {
    fn default() -> Self {
        Self::new()
    }
}
