//! Lecteur polyphonique du soundboard. Les sons sont des tampons stereo entrelaces a 48 kHz,
//! deja decodes. Aucune allocation : les commandes arrivent par une file externe.

use std::sync::Arc;

use crate::SAMPLE_RATE;

pub const MAX_PADS: usize = 64;
/// Chaque pad peut avoir une voix en cours plus une voix en fondu de sortie.
const MAX_VOICES: usize = MAX_PADS * 2;
/// Duree du fondu de sortie (arret ou relance), en secondes.
const FADE_OUT_S: f32 = 0.005;

pub enum PadCommand {
    /// Joue `data` depuis le debut avec le gain lineaire `gain` ; relance le pad s'il joue deja.
    Play { pad: usize, data: Arc<[f32]>, gain: f32 },
    Stop(usize),
    StopAll,
}

struct Voice {
    pad: usize,
    data: Arc<[f32]>,
    /// Indice d'echantillon (pair : debut de trame).
    pos: usize,
    gain: f32,
    /// Trames restantes du fondu de sortie, `None` tant que la voix joue normalement.
    fade_left: Option<u32>,
}

pub struct Player {
    voices: Vec<Option<Voice>>,
    fade_frames: u32,
    /// Commandes ignorees faute de voix libre.
    pub dropped: u32,
}

impl Player {
    pub fn new() -> Self {
        Self {
            voices: (0..MAX_VOICES).map(|_| None).collect(),
            fade_frames: (FADE_OUT_S * SAMPLE_RATE) as u32,
            dropped: 0,
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().flatten().count()
    }

    fn start_fade(&mut self, pad: Option<usize>) {
        let frames = self.fade_frames;
        for v in self.voices.iter_mut().flatten() {
            if (pad.is_none() || pad == Some(v.pad)) && v.fade_left.is_none() {
                v.fade_left = Some(frames);
            }
        }
    }

    pub fn apply(&mut self, cmd: PadCommand) {
        match cmd {
            PadCommand::Play { pad, data, gain } => {
                self.start_fade(Some(pad));
                match self.voices.iter_mut().find(|v| v.is_none()) {
                    Some(slot) => *slot = Some(Voice { pad, data, pos: 0, gain, fade_left: None }),
                    None => self.dropped += 1,
                }
            }
            PadCommand::Stop(pad) => self.start_fade(Some(pad)),
            PadCommand::StopAll => self.start_fade(None),
        }
    }

    /// Ajoute les voix actives au bloc stereo entrelace, multipliees par `master` (lineaire).
    pub fn mix_into(&mut self, buf: &mut [f32], master: f32) {
        let fade_frames = self.fade_frames.max(1) as f32;
        for slot in &mut self.voices {
            let Some(v) = slot else { continue };
            let mut finished = false;
            for frame in buf.as_chunks_mut::<2>().0 {
                if v.pos + 1 >= v.data.len() {
                    finished = true;
                    break;
                }
                let mut g = v.gain * master;
                if let Some(left) = v.fade_left.as_mut() {
                    if *left == 0 {
                        finished = true;
                        break;
                    }
                    g *= *left as f32 / fade_frames;
                    *left -= 1;
                }
                frame[0] += v.data[v.pos] * g;
                frame[1] += v.data[v.pos + 1] * g;
                v.pos += 2;
            }
            if finished {
                *slot = None;
            }
        }
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(frames: usize, value: f32) -> Arc<[f32]> {
        vec![value; frames * 2].into()
    }

    #[test]
    fn first_sample_lands_in_next_block() {
        let mut p = Player::new();
        p.apply(PadCommand::Play { pad: 0, data: pad(100, 0.5), gain: 1.0 });
        let mut buf = vec![0.0f32; 2 * 64];
        p.mix_into(&mut buf, 1.0);
        assert_eq!(buf[0], 0.5);
        assert_eq!(buf[1], 0.5);
    }

    #[test]
    fn adds_to_existing_signal_and_ends() {
        let mut p = Player::new();
        p.apply(PadCommand::Play { pad: 1, data: pad(10, 0.25), gain: 1.0 });
        let mut buf = vec![0.1f32; 2 * 32];
        p.mix_into(&mut buf, 1.0);
        assert!((buf[0] - 0.35).abs() < 1e-6);
        assert!((buf[2 * 20] - 0.1).abs() < 1e-6, "apres la fin, la voix est intacte");
        assert_eq!(p.active_voices(), 0);
    }

    #[test]
    fn volumes_multiply() {
        let mut p = Player::new();
        p.apply(PadCommand::Play { pad: 0, data: pad(10, 1.0), gain: 0.5 });
        let mut buf = vec![0.0f32; 2 * 4];
        p.mix_into(&mut buf, 0.5);
        assert!((buf[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn retrigger_restarts_from_beginning() {
        let mut p = Player::new();
        let data: Arc<[f32]> = (0..2 * 1000).map(|i| (i / 2) as f32 / 1000.0).collect::<Vec<_>>().into();
        p.apply(PadCommand::Play { pad: 3, data: data.clone(), gain: 1.0 });
        let mut buf = vec![0.0f32; 2 * 500];
        p.mix_into(&mut buf, 1.0);
        p.apply(PadCommand::Play { pad: 3, data, gain: 1.0 });
        let mut buf = vec![0.0f32; 2 * 4];
        p.mix_into(&mut buf, 1.0);
        // La nouvelle voix repart de 0 ; l'ancienne finit en fondu (somme proche de 0.5 au debut).
        assert!(buf[0] > 0.4 && buf[0] < 0.6, "{}", buf[0]);
        let mut rest = vec![0.0f32; 2 * 500];
        p.mix_into(&mut rest, 1.0);
        assert_eq!(p.active_voices(), 1, "l'ancienne voix a fini son fondu");
    }

    #[test]
    fn stop_all_fades_out_then_silences() {
        let mut p = Player::new();
        p.apply(PadCommand::Play { pad: 0, data: pad(48_000, 0.5), gain: 1.0 });
        p.apply(PadCommand::Play { pad: 1, data: pad(48_000, 0.5), gain: 1.0 });
        p.apply(PadCommand::StopAll);
        let mut buf = vec![0.0f32; 2 * 480];
        p.mix_into(&mut buf, 1.0);
        assert!(buf[0] <= 1.0 && buf[0] > 0.0);
        assert!(buf[2 * 400].abs() < 1e-6, "silence apres le fondu");
        assert_eq!(p.active_voices(), 0);
    }

    #[test]
    fn sixteen_pads_play_together() {
        let mut p = Player::new();
        for i in 0..16 {
            p.apply(PadCommand::Play { pad: i, data: pad(1000, 0.01), gain: 1.0 });
        }
        let mut buf = vec![0.0f32; 2 * 10];
        p.mix_into(&mut buf, 1.0);
        assert!((buf[0] - 0.16).abs() < 1e-5);
        assert_eq!(p.dropped, 0);
    }
}
