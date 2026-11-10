use crate::SAMPLE_RATE;
use crate::effect::Effect;
use crate::param::db_to_lin;

/// Constante de temps du lissage du gain, en secondes (evite les clics).
const SMOOTHING_S: f32 = 0.005;

/// Gain en dB avec lissage vers la valeur cible.
pub struct Gain {
    target: f32,
    current: f32,
    coeff: f32,
}

impl Gain {
    pub fn new(db: f32) -> Self {
        let target = db_to_lin(db);
        let coeff = 1.0 - (-1.0 / (SMOOTHING_S * SAMPLE_RATE)).exp();
        Self { target, current: target, coeff }
    }

    pub fn set_db(&mut self, db: f32) {
        self.target = db_to_lin(db);
    }
}

impl Effect for Gain {
    fn process(&mut self, buf: &mut [f32], channels: usize) {
        for frame in buf.chunks_mut(channels.max(1)) {
            self.current += (self.target - self.current) * self.coeff;
            for s in frame {
                *s *= self.current;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{peak_dbfs, sine};

    fn run(gain_db: f32, input_dbfs: f32) -> f32 {
        let mut g = Gain::new(gain_db);
        let mut buf = sine(1000.0, input_dbfs, 4800, 1);
        g.process(&mut buf, 1);
        peak_dbfs(&buf[2400..])
    }

    #[test]
    fn plus_six_db() {
        let out = run(6.0, -20.0);
        assert!((out - (-14.0)).abs() < 0.1, "sortie {out} dBFS");
    }

    #[test]
    fn zero_db_is_transparent() {
        let out = run(0.0, -20.0);
        assert!((out - (-20.0)).abs() < 0.05, "sortie {out} dBFS");
    }

    #[test]
    fn change_is_smoothed() {
        let mut g = Gain::new(0.0);
        g.set_db(12.0);
        let mut buf = vec![0.5f32; 2400];
        g.process(&mut buf, 1);
        assert!(buf[0] < 0.6, "pas de saut immediat");
        assert!(buf[2399] > 1.9, "atteint la cible");
    }
}
