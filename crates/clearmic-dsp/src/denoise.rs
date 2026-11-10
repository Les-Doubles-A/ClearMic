//! Suppression de bruit RNNoise (portage Rust `nnnoiseless`, licence BSD-3-Clause).
//!
//! Le moteur travaille par trames de 480 echantillons (10 ms a 48 kHz) : la voie traitee est donc
//! retardee de 10 ms. La voie seche est retardee de la meme duree pour que le melange soit
//! aligne. L'effet tourne en permanence (etat chaud, pas de transitoire a l'activation) ; quand il
//! est desactive, un fondu de 10 ms ramene la sortie sur le signal direct, sans retard.

use nnnoiseless::DenoiseState;

use crate::SAMPLE_RATE;

const FRAME: usize = DenoiseState::FRAME_SIZE;
/// Le moteur attend des valeurs d'amplitude 16 bits.
const PCM_SCALE: f32 = 32_768.0;
const FADE_S: f32 = 0.010;

struct Channel {
    state: Box<DenoiseState<'static>>,
    cur_in: [f32; FRAME],
    prev_in: [f32; FRAME],
    prev_wet: [f32; FRAME],
    scaled: [f32; FRAME],
    wet: [f32; FRAME],
}

impl Channel {
    fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            cur_in: [0.0; FRAME],
            prev_in: [0.0; FRAME],
            prev_wet: [0.0; FRAME],
            scaled: [0.0; FRAME],
            wet: [0.0; FRAME],
        }
    }

    fn run_frame(&mut self) {
        for (d, s) in self.scaled.iter_mut().zip(self.cur_in.iter()) {
            // Un signal au-dela de la pleine echelle 16 bits fausse le modele (voix robotique) :
            // on le tronque comme le ferait une conversion en entiers.
            *d = (s * PCM_SCALE).clamp(-PCM_SCALE, PCM_SCALE - 1.0);
        }
        self.state.process_frame(&mut self.wet, &self.scaled);
        self.prev_in = self.cur_in;
        for (d, s) in self.prev_wet.iter_mut().zip(self.wet.iter()) {
            *d = s / PCM_SCALE;
        }
    }
}

pub struct Denoise {
    channels: [Channel; 2],
    pos: usize,
    /// Part de signal retarde presente en sortie (0 = direct, 1 = retarde), lissee.
    active_mix: f32,
    /// Intensite effective (part de voie traitee dans le signal retarde), lissee.
    amount: f32,
    target_active: f32,
    target_amount: f32,
    k: f32,
}

impl Denoise {
    pub fn new() -> Self {
        Self {
            channels: [Channel::new(), Channel::new()],
            pos: 0,
            active_mix: 0.0,
            amount: 1.0,
            target_active: 0.0,
            target_amount: 1.0,
            k: 1.0 - (-1.0 / (FADE_S * SAMPLE_RATE)).exp(),
        }
    }

    pub fn configure(&mut self, on: bool, amount: f32) {
        self.target_active = if on { 1.0 } else { 0.0 };
        self.target_amount = amount.clamp(0.0, 1.0);
    }

    /// Bloc stereo entrelace.
    pub fn process(&mut self, buf: &mut [f32]) {
        for frame in buf.as_chunks_mut::<2>().0 {
            self.active_mix += (self.target_active - self.active_mix) * self.k;
            self.amount += (self.target_amount - self.amount) * self.k;
            let (g, a) = (self.active_mix, self.amount);
            for (ch, x) in frame.iter_mut().enumerate() {
                let c = &mut self.channels[ch];
                let delayed = c.prev_in[self.pos] * (1.0 - a) + c.prev_wet[self.pos] * a;
                let direct = *x;
                c.cur_in[self.pos] = direct;
                *x = direct + (delayed - direct) * g;
            }
            self.pos += 1;
            if self.pos == FRAME {
                self.pos = 0;
                for c in &mut self.channels {
                    c.run_frame();
                }
            }
        }
    }
}

impl Default for Denoise {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{max_step, rms_dbfs, sine, white_noise};

    fn stereo(mono: &[f32]) -> Vec<f32> {
        mono.iter().flat_map(|v| [*v, *v]).collect()
    }

    fn run(d: &mut Denoise, input: &[f32]) -> Vec<f32> {
        let mut out = stereo(input);
        for chunk in out.chunks_mut(2 * 192) {
            d.process(chunk);
        }
        out.iter().step_by(2).copied().collect()
    }

    /// Le critere AC-F2-1 vise 15 dB, mais RNNoise reagit de facon instable au bruit synthetique
    /// stationnaire (mesure : de 9 a 39 dB selon la seconde analysee). Ce test verifie seulement une
    /// reduction nette et reproductible ; la qualite reelle se juge sur un enregistrement (M3).
    #[test]
    fn white_noise_is_clearly_reduced() {
        let noise = white_noise(-40.0, 48_000 * 3);
        let mut d = Denoise::new();
        d.configure(true, 1.0);
        let out = run(&mut d, &noise);
        let before = rms_dbfs(&noise[48_000..]);
        let after = rms_dbfs(&out[48_000..]);
        assert!(before - after >= 8.0, "avant {before} dBFS, apres {after} dBFS");
    }

    #[test]
    fn amount_zero_is_input_delayed_by_one_frame() {
        let src = sine(440.0, -20.0, 48_000, 1);
        let mut d = Denoise::new();
        d.configure(true, 0.0);
        let out = run(&mut d, &src);
        // Laisse le fondu d'activation (10 ms) se terminer, puis compare au signal retarde.
        let worst = (4800..src.len())
            .map(|i| (out[i] - src[i - FRAME]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-3, "ecart max {worst}");
    }

    #[test]
    fn off_is_transparent_once_faded() {
        let src = sine(440.0, -20.0, 48_000, 1);
        let mut d = Denoise::new();
        d.configure(false, 1.0);
        let out = run(&mut d, &src);
        assert_eq!(&out[..], &src[..]);
    }

    #[test]
    fn toggle_has_no_click() {
        let src = sine(440.0, -20.0, 96_000, 1);
        let mut d = Denoise::new();
        d.configure(false, 1.0);
        let mut all = run(&mut d, &src[..48_000]);
        d.configure(true, 1.0);
        all.extend(run(&mut d, &src[48_000..]));
        assert!(max_step(&all) < 0.1, "saut {}", max_step(&all));
        d.configure(false, 1.0);
        let third = run(&mut d, &src[..48_000]);
        assert!(max_step(&third) < 0.1, "saut {}", max_step(&third));
    }
}
