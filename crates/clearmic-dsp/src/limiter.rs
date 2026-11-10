//! Etage de sortie : limiteur a anticipation plafonne a -1 dBFS, ou simple troncature a 0 dBFS
//! avec indicateur.
//!
//! Un limiteur a attaque instantanee cree une marche dans le gain a chaque crete, ce qui s'entend
//! comme un son metallique des que le boost est eleve. Ici le signal est retarde de `LOOKAHEAD - 1`
//! echantillons et le gain descend en rampe de `LOOKAHEAD` echantillons, de sorte qu'il atteint la
//! valeur requise exactement quand la crete arrive. Le retard est constant, que le limiteur soit
//! actif ou non, pour qu'aucun saut de timing n'accompagne la bascule.

use crate::SAMPLE_RATE;

/// Plafond du limiteur : -1 dBFS (0,891, arrondi vers le bas).
pub const CEILING: f32 = 0.891;
/// Duree de l'anticipation et de la rampe d'attaque, en echantillons (1,33 ms).
pub const LOOKAHEAD: usize = 64;
/// Retard ajoute par le limiteur, en echantillons.
pub const DELAY_FRAMES: usize = LOOKAHEAD - 1;
const RELEASE_S: f32 = 0.080;

pub struct Limiter {
    /// Signal retarde, une trame stereo par case.
    audio: [[f32; 2]; LOOKAHEAD],
    /// Gain requis par trame, pour le minimum glissant.
    need: [f32; LOOKAHEAD],
    /// Minimum glissant du gain requis, pour la moyenne glissante.
    mins: [f32; LOOKAHEAD],
    sum: f32,
    pos: usize,
    gain: f32,
    release: f32,
}

impl Limiter {
    pub fn new() -> Self {
        Self {
            audio: [[0.0; 2]; LOOKAHEAD],
            need: [1.0; LOOKAHEAD],
            mins: [1.0; LOOKAHEAD],
            sum: LOOKAHEAD as f32,
            pos: 0,
            gain: 1.0,
            release: 1.0 - (-1.0 / (RELEASE_S * SAMPLE_RATE)).exp(),
        }
    }

    /// Traite un bloc stereo entrelace. `limit` actif : aucune valeur ne depasse `CEILING`.
    /// Inactif : troncature a +-1.0. Renvoie vrai si au moins un echantillon a ete tronque.
    pub fn process(&mut self, buf: &mut [f32], limit: bool) -> bool {
        let mut clipped = false;
        for frame in buf.as_chunks_mut::<2>().0 {
            let mut input = *frame;
            for s in input.iter_mut() {
                if !s.is_finite() {
                    *s = 0.0;
                }
            }
            let peak = input[0].abs().max(input[1].abs());
            let need = if limit && peak > CEILING { CEILING / peak } else { 1.0 };

            // La trame sortante est celle entree il y a `LOOKAHEAD - 1` pas : toute la fenetre de
            // moyenne (les `LOOKAHEAD` derniers minimums) la couvre, donc le gain l'a deja atteinte.
            self.audio[self.pos] = input;
            self.need[self.pos] = need;
            let out_audio = self.audio[(self.pos + 1) % LOOKAHEAD];

            // Minimum sur la fenetre, puis moyenne glissante de ces minimums.
            let min = self.need.iter().fold(1.0f32, |m, v| m.min(*v));
            self.sum += min - self.mins[self.pos];
            self.mins[self.pos] = min;
            self.pos = (self.pos + 1) % LOOKAHEAD;
            let avg = (self.sum / LOOKAHEAD as f32).min(1.0);

            // Descente immediate (elle est deja en rampe), remontee progressive.
            self.gain = if avg < self.gain { avg } else { self.gain + (avg - self.gain) * self.release };

            if limit {
                for (o, s) in frame.iter_mut().zip(out_audio) {
                    *o = (s * self.gain).clamp(-CEILING, CEILING);
                }
            } else {
                for (o, s) in frame.iter_mut().zip(out_audio) {
                    if s.abs() > 1.0 {
                        clipped = true;
                    }
                    *o = s.clamp(-1.0, 1.0);
                }
            }
        }
        clipped
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param::db_to_lin;
    use crate::testutil::sine;

    #[test]
    fn limiter_never_exceeds_ceiling() {
        let mut l = Limiter::new();
        let mut buf = sine(1000.0, -6.0, 9600, 2);
        buf.iter_mut().for_each(|s| *s *= db_to_lin(20.0));
        l.process(&mut buf, true);
        let max = buf.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(max <= 0.891, "crete {max}");
        assert!(max > 0.85, "le signal ne doit pas etre ecrase : {max}");
    }

    #[test]
    fn quiet_signal_passes_unchanged_after_the_delay() {
        let mut l = Limiter::new();
        let src = sine(1000.0, -20.0, 960, 2);
        let mut buf = src.clone();
        l.process(&mut buf, true);
        let d = 2 * DELAY_FRAMES;
        assert_eq!(&src[..src.len() - d], &buf[d..]);
    }

    #[test]
    fn limiter_off_clips_and_flags() {
        let mut l = Limiter::new();
        let mut buf = vec![0.0f32; 2 * LOOKAHEAD + 6];
        let n = buf.len();
        buf[n - 6..].copy_from_slice(&[0.5, 3.0, -2.0, f32::NAN, f32::INFINITY, 0.0]);
        let mut tail = vec![0.0f32; 2 * LOOKAHEAD];
        let a = l.process(&mut buf, false);
        let b = l.process(&mut tail, false);
        assert!(a || b);
        let all: Vec<f32> = buf.iter().chain(tail.iter()).copied().collect();
        let d = 2 * DELAY_FRAMES;
        assert_eq!(&all[n - 6 + d..n + d], &[0.5, 1.0, -1.0, 0.0, 0.0, 0.0]);
        assert!(all.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn no_flag_when_nothing_clips() {
        let mut l = Limiter::new();
        let mut buf = sine(1000.0, -3.0, 480, 2);
        assert!(!l.process(&mut buf, false));
    }

    /// Un son bref tres fort doit etre ramene sous le plafond sans marche dans le gain : le rapport
    /// sortie / entree retardee varie de moins de 3 % d'un echantillon a l'autre.
    #[test]
    fn gain_changes_in_a_ramp_not_a_step() {
        let mut l = Limiter::new();
        let mut src = vec![0.0f32; 2 * 4800];
        for (i, frame) in src.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let burst = if (2400..2640).contains(&i) { 4.0 } else { 0.05 };
            let v = (2.0 * std::f32::consts::PI * 300.0 * i as f32 / SAMPLE_RATE).sin() * burst;
            *frame = [v, v];
        }
        let mut buf = src.clone();
        l.process(&mut buf, true);
        assert!(buf.iter().all(|s| s.abs() <= CEILING));
        let d = DELAY_FRAMES;
        let gains: Vec<(usize, f32)> = (d..4800)
            .filter(|i| src[2 * (i - d)].abs() > 0.02)
            .map(|i| (i, buf[2 * i] / src[2 * (i - d)]))
            .collect();
        // Variation par echantillon, ecart de position compris (le signal passe par zero).
        let worst = gains.windows(2).map(|w| (w[1].1 - w[0].1).abs() / (w[1].0 - w[0].0) as f32).fold(0.0f32, f32::max);
        assert!(worst < 0.03, "saut de gain {worst}");
        assert!(gains.iter().map(|g| g.1).fold(1.0f32, f32::min) < 0.5, "la limitation doit agir");
    }
}
