//! Repartition des canaux : mono, balance et largeur stereo (matrice M/S).

use crate::SAMPLE_RATE;

const SMOOTHING_S: f32 = 0.005;

pub struct Channels {
    /// Valeurs lissees : largeur (1.0 = inchange), gains gauche et droite de la balance, part mono.
    width: f32,
    gl: f32,
    gr: f32,
    mono_mix: f32,
    target_width: f32,
    target_gl: f32,
    target_gr: f32,
    target_mono: f32,
    k: f32,
}

impl Channels {
    pub fn new() -> Self {
        Self {
            width: 1.0,
            gl: 1.0,
            gr: 1.0,
            mono_mix: 0.0,
            target_width: 1.0,
            target_gl: 1.0,
            target_gr: 1.0,
            target_mono: 0.0,
            k: 1.0 - (-1.0 / (SMOOTHING_S * SAMPLE_RATE)).exp(),
        }
    }

    /// `balance` de -100 a +100, `width` de 0 a 200 (pour cent).
    pub fn configure(&mut self, mono: bool, balance: f32, width: f32) {
        self.target_mono = if mono { 1.0 } else { 0.0 };
        self.target_width = width.clamp(0.0, 200.0) / 100.0;
        let b = balance.clamp(-100.0, 100.0) / 100.0;
        self.target_gl = if b > 0.0 { 1.0 - b } else { 1.0 };
        self.target_gr = if b < 0.0 { 1.0 + b } else { 1.0 };
    }

    /// Bloc stereo entrelace.
    pub fn process(&mut self, buf: &mut [f32]) {
        for frame in buf.as_chunks_mut::<2>().0 {
            self.width += (self.target_width - self.width) * self.k;
            self.gl += (self.target_gl - self.gl) * self.k;
            self.gr += (self.target_gr - self.gr) * self.k;
            self.mono_mix += (self.target_mono - self.mono_mix) * self.k;
            let (l, r) = (frame[0], frame[1]);
            let mid = (l + r) * 0.5;
            // En mono, la largeur et la balance ne s'appliquent plus : L = R = (L + R) / 2.
            let side = (l - r) * 0.5 * self.width * (1.0 - self.mono_mix);
            let gl = self.gl + (1.0 - self.gl) * self.mono_mix;
            let gr = self.gr + (1.0 - self.gr) * self.mono_mix;
            frame[0] = (mid + side) * gl;
            frame[1] = (mid - side) * gr;
        }
    }
}

impl Default for Channels {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(c: &mut Channels) {
        let mut warm = vec![0.0f32; 2 * 4800];
        c.process(&mut warm);
    }

    #[test]
    fn mono_averages_channels() {
        let mut c = Channels::new();
        c.configure(true, 80.0, 200.0);
        settle(&mut c);
        let mut b = [0.8f32, 0.2, -0.4, 0.2];
        c.process(&mut b);
        assert!((b[0] - 0.5).abs() < 1e-4 && (b[1] - 0.5).abs() < 1e-4);
        assert!((b[2] + 0.1).abs() < 1e-4 && (b[3] + 0.1).abs() < 1e-4);
    }

    #[test]
    fn balance_extremes_silence_one_side() {
        let mut c = Channels::new();
        c.configure(false, -100.0, 100.0);
        settle(&mut c);
        let mut b = [0.5f32, 0.5];
        c.process(&mut b);
        assert!((b[0] - 0.5).abs() < 1e-4 && b[1].abs() < 1e-4);

        c.configure(false, 100.0, 100.0);
        settle(&mut c);
        let mut b = [0.5f32, 0.5];
        c.process(&mut b);
        assert!(b[0].abs() < 1e-4 && (b[1] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn centered_mono_source_stays_equal() {
        let mut c = Channels::new();
        c.configure(false, 0.0, 100.0);
        settle(&mut c);
        let mut b = [0.3f32, 0.3];
        c.process(&mut b);
        assert_eq!(b[0], b[1]);
        assert!((b[0] - 0.3).abs() < 1e-5);
    }

    #[test]
    fn width_zero_is_mono_and_200_doubles_side() {
        let mut c = Channels::new();
        c.configure(false, 0.0, 0.0);
        settle(&mut c);
        let mut b = [0.8f32, 0.2];
        c.process(&mut b);
        assert!((b[0] - 0.5).abs() < 1e-4 && (b[1] - 0.5).abs() < 1e-4);

        c.configure(false, 0.0, 100.0);
        settle(&mut c);
        let mut b = [0.8f32, 0.2];
        c.process(&mut b);
        assert!((b[0] - 0.8).abs() < 1e-4 && (b[1] - 0.2).abs() < 1e-4);

        c.configure(false, 0.0, 200.0);
        settle(&mut c);
        let mut b = [0.8f32, 0.2];
        c.process(&mut b);
        // mid 0.5, side 0.3 doublee -> 0.6 : L = 1.1, R = -0.1
        assert!((b[0] - 1.1).abs() < 1e-4 && (b[1] + 0.1).abs() < 1e-4);
    }
}
