use std::sync::atomic::{AtomicU32, Ordering};

/// Parametre f32 partage entre le thread d'interface et le rappel audio, sans verrou.
#[derive(Debug)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

impl Default for AtomicF32 {
    fn default() -> Self {
        Self::new(0.0)
    }
}

/// Decibels vers rapport d'amplitude.
pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Rapport d'amplitude vers decibels (plancher -200 dB pour 0).
pub fn lin_to_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-10).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_roundtrip() {
        let p = AtomicF32::new(1.5);
        assert_eq!(p.get(), 1.5);
        p.set(-3.25);
        assert_eq!(p.get(), -3.25);
    }

    #[test]
    fn db_conversion() {
        assert!((db_to_lin(6.0206) - 2.0).abs() < 1e-3);
        assert!((lin_to_db(0.5) + 6.0206).abs() < 1e-3);
        assert!(lin_to_db(0.0) <= -199.0);
    }
}
