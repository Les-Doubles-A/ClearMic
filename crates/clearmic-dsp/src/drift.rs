//! Absorbe la derive d'horloge entre le flux de capture et le flux de sortie.
//!
//! Le micro et la sortie ont des horloges distinctes. La file qui les relie se vide ou se
//! remplit lentement. Le controleur observe le remplissage minimal sur une fenetre de blocs
//! (le minimum est insensible a la phase des rappels) et corrige de quelques trames a la fois.

/// Profil faible latence : sortie en blocs de 128 trames, entree toujours en blocs de 480.
/// Marge visee apres lecture, en trames (3 ms : couvre la gigue des rappels du micro).
pub const LOW_LATENCY_TARGET: usize = 144;
pub const LOW_LATENCY_SLACK: usize = 64;
/// Le remplissage amorce doit couvrir un bloc d'entree complet : l'amorcage se termine juste apres
/// l'arrivee d'un bloc, donc au sommet de la dent de scie.
pub const LOW_LATENCY_PRIME_EXTRA: usize = 480;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillAction {
    /// File pas encore amorcee : sortir du silence (pas un incident).
    Priming,
    /// Pas assez de donnees pour le bloc : silence, puis reamorcage (incident compte).
    Underrun,
    /// Lire le bloc. `skip` trames sont ignorees avant la lecture (file trop pleine),
    /// `repeat` trames sont lues en moins et la derniere est repetee (file trop vide).
    Play { skip: usize, repeat: usize },
}

#[derive(Debug, Clone)]
pub struct FillController {
    /// Marge visee (trames restantes apres lecture, au minimum de la fenetre).
    target: usize,
    /// Tolerance autour de la cible avant correction.
    slack: usize,
    /// Nombre de blocs par fenetre d'observation.
    window: usize,
    /// Correction maximale par fenetre, en trames.
    max_adjust: usize,
    primed: bool,
    seen: usize,
    min_extra: usize,
    /// Remplissage supplementaire exige avant de sortir du silence (voir `with_prime_extra`).
    prime_extra: usize,
}

impl FillController {
    pub fn new(target: usize, slack: usize, window: usize) -> Self {
        Self {
            target,
            slack,
            window: window.max(1),
            max_adjust: 16,
            primed: false,
            seen: 0,
            min_extra: usize::MAX,
            prime_extra: 0,
        }
    }

    /// Exige `extra` trames de plus avant l'amorcage (blocs d'entree plus gros que ceux de sortie).
    pub fn with_prime_extra(mut self, extra: usize) -> Self {
        self.prime_extra = extra;
        self
    }

    pub fn decide(&mut self, available: usize, requested: usize) -> FillAction {
        if !self.primed {
            if available < requested + self.target + self.prime_extra {
                return FillAction::Priming;
            }
            self.primed = true;
            self.reset_window();
        }
        if available < requested {
            self.primed = false;
            return FillAction::Underrun;
        }
        self.min_extra = self.min_extra.min(available - requested);
        self.seen += 1;
        if self.seen < self.window {
            return FillAction::Play { skip: 0, repeat: 0 };
        }
        let min_extra = self.min_extra;
        self.reset_window();
        if min_extra > self.target + self.slack {
            let skip = (min_extra - self.target).min(self.max_adjust);
            FillAction::Play { skip, repeat: 0 }
        } else if min_extra + self.slack < self.target {
            let repeat = (self.target - min_extra)
                .min(self.max_adjust)
                .min(requested.saturating_sub(1));
            FillAction::Play { skip: 0, repeat }
        } else {
            FillAction::Play { skip: 0, repeat: 0 }
        }
    }

    fn reset_window(&mut self) {
        self.seen = 0;
        self.min_extra = usize::MAX;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: usize = 480;
    /// Marge visee : une trame de capture plus 2 ms de gigue.
    const TARGET: usize = CHUNK + 96;

    /// Simule 2 horloges. `ppm` : l'entree tourne plus vite (>0) ou plus lentement (<0).
    /// L'entree livre des blocs de `CHUNK` trames, la sortie en lit `out_chunk`.
    /// Retourne (sous-alimentations apres amorcage, remplissage max, corrections).
    fn simulate_with(ppm: f64, seconds: f64, jitter_frames: f64, out_chunk: usize, target: usize, slack: usize, prime_extra: usize) -> (u32, usize, u32) {
        let mut ctl = FillController::new(target, slack, 50).with_prime_extra(prime_extra);
        let rate_in = 48_000.0 * (1.0 + ppm / 1e6);
        let t_in = CHUNK as f64 / rate_in;
        let t_out = out_chunk as f64 / 48_000.0;
        let mut rng: u32 = 7;
        let mut jitter = move || {
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((rng >> 8) as f64 / 16_777_216.0 * 2.0 - 1.0) * jitter_frames / 48_000.0
        };
        let (mut next_in, mut next_out) = (0.0f64, t_out * 0.37);
        let (mut i, mut o) = (0u64, 0u64);
        let mut fill = 0usize;
        let (mut underruns, mut max_fill, mut corrections) = (0u32, 0usize, 0u32);
        let mut primed_once = false;
        while next_in < seconds && next_out < seconds {
            if next_in <= next_out {
                fill += CHUNK;
                max_fill = max_fill.max(fill);
                i += 1;
                next_in = i as f64 * t_in + jitter();
            } else {
                match ctl.decide(fill, out_chunk) {
                    FillAction::Priming => {}
                    FillAction::Underrun => {
                        if primed_once {
                            underruns += 1;
                        }
                    }
                    FillAction::Play { skip, repeat } => {
                        primed_once = true;
                        if skip + repeat > 0 {
                            corrections += 1;
                        }
                        fill -= skip + out_chunk - repeat;
                    }
                }
                o += 1;
                next_out = o as f64 * t_out + t_out * 0.37 + jitter();
            }
        }
        (underruns, max_fill, corrections)
    }

    fn simulate(ppm: f64, seconds: f64, jitter_frames: f64) -> (u32, usize, u32) {
        simulate_with(ppm, seconds, jitter_frames, CHUNK, TARGET, 96, 0)
    }

    #[test]
    fn low_latency_profile_survives_drift_and_jitter() {
        for ppm in [0.0, 200.0, -200.0, 1000.0] {
            let (u, max_fill, _) = simulate_with(ppm, 300.0, 48.0, 128, LOW_LATENCY_TARGET, LOW_LATENCY_SLACK, LOW_LATENCY_PRIME_EXTRA);
            assert_eq!(u, 0, "{ppm} ppm : sous-alimentations");
            assert!(max_fill < 4 * CHUNK, "{ppm} ppm : remplissage max {max_fill}");
        }
    }

    #[test]
    fn perfect_clocks_no_underrun() {
        let (u, _, _) = simulate(0.0, 600.0, 24.0);
        assert_eq!(u, 0);
    }

    #[test]
    fn fast_input_is_drained() {
        let (u, max_fill, c) = simulate(200.0, 600.0, 24.0);
        assert_eq!(u, 0);
        assert!(max_fill < 4 * CHUNK, "remplissage max {max_fill}");
        assert!(c > 0, "des corrections doivent avoir lieu");
    }

    #[test]
    fn slow_input_is_stretched() {
        let (u, _, c) = simulate(-200.0, 600.0, 24.0);
        assert_eq!(u, 0);
        assert!(c > 0);
    }

    #[test]
    fn priming_waits_for_target() {
        let mut c = FillController::new(TARGET, 96, 10);
        assert_eq!(c.decide(480, 480), FillAction::Priming);
        assert_eq!(c.decide(480 + TARGET, 480), FillAction::Play { skip: 0, repeat: 0 });
    }

    #[test]
    fn underrun_reprimes() {
        let mut c = FillController::new(TARGET, 96, 10);
        c.decide(2000, 480);
        assert_eq!(c.decide(100, 480), FillAction::Underrun);
        assert_eq!(c.decide(480, 480), FillAction::Priming);
    }
}
