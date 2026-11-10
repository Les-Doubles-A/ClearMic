//! Estimation de la latence micro vers sortie.

use crate::SAMPLE_RATE;

/// Latence algorithmique du debruitage RNNoise (une trame de 480 echantillons a 48 kHz).
pub const DENOISE_LATENCY_MS: f32 = 10.0;

/// Retard de l'anticipation du limiteur.
pub const LIMITER_LATENCY_MS: f32 = crate::limiter::DELAY_FRAMES as f32 / SAMPLE_RATE * 1000.0;

fn frames_to_ms(frames: f32) -> f32 {
    frames / SAMPLE_RATE * 1000.0
}

/// Estimation en millisecondes : une trame de capture, la file de decouplage, une trame de
/// sortie, l'anticipation du limiteur, plus le debruitage si actif. Exclut le tampon interne du pilote du micro virtuel.
pub fn estimate_latency_ms(in_chunk: u32, out_chunk: u32, fill_frames: f32, denoise_active: bool) -> f32 {
    let base = frames_to_ms(in_chunk as f32 + out_chunk as f32 + fill_frames);
    base + LIMITER_LATENCY_MS + if denoise_active { DENOISE_LATENCY_MS } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_chunks_and_fill() {
        let ms = estimate_latency_ms(480, 480, 144.0, false);
        assert!((ms - 23.0 - LIMITER_LATENCY_MS).abs() < 1e-3, "{ms}");
    }

    #[test]
    fn denoise_adds_ten_ms() {
        let a = estimate_latency_ms(480, 480, 0.0, false);
        let b = estimate_latency_ms(480, 480, 0.0, true);
        assert!((b - a - 10.0).abs() < 1e-3);
    }
}
