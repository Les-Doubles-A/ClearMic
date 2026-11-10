//! Traitement du signal de ClearMic, sans dependance audio systeme.

#![cfg_attr(test, allow(clippy::field_reassign_with_default))]

pub mod biquad;
pub mod chain;
pub mod channels;
pub mod compressor;
pub mod denoise;
pub mod drift;
pub mod effect;
pub mod eq;
pub mod gain;
pub mod latency;
pub mod limiter;
pub mod param;
pub mod player;
pub mod processor;
pub mod settings;
pub mod testutil;

/// Frequence d'echantillonnage interne (H5).
pub const SAMPLE_RATE: f32 = 48_000.0;
