//! Chaine complete : gain d'entree, debruitage, passe-haut et egaliseur, compresseur, canaux,
//! boost, puis (separement) mixage du soundboard et limiteur de sortie.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::channels::Channels;
use crate::compressor::Compressor;
use crate::denoise::Denoise;
use crate::effect::Effect;
use crate::eq::Equalizer;
use crate::gain::Gain;
use crate::limiter::Limiter;
use crate::param::db_to_lin;
use crate::player::{PadCommand, Player};
use crate::settings::{Meters, Settings};

pub struct Processor {
    input_gain: Gain,
    denoise: Denoise,
    eq: Equalizer,
    comp: Compressor,
    comp_was_on: bool,
    channels: Channels,
    boost: Gain,
    limiter: Limiter,
    player: Player,
    meters: Arc<Meters>,
}

impl Processor {
    pub fn new(meters: Arc<Meters>) -> Self {
        Self {
            input_gain: Gain::new(0.0),
            denoise: Denoise::new(),
            eq: Equalizer::new(),
            comp: Compressor::new(),
            comp_was_on: false,
            channels: Channels::new(),
            boost: Gain::new(0.0),
            limiter: Limiter::new(),
            player: Player::new(),
            meters,
        }
    }

    pub fn apply_pad_command(&mut self, cmd: PadCommand) {
        self.player.apply(cmd);
    }

    /// Voix seule (stereo entrelace) : tout sauf le soundboard et le limiteur.
    pub fn process_voice(&mut self, buf: &mut [f32], s: &Settings) {
        self.input_gain.set_db(s.input_gain_db);
        self.input_gain.process(buf, 2);

        self.denoise.configure(s.denoise_on, s.denoise_amount);
        self.denoise.process(buf);

        self.eq.configure(s);
        self.eq.process(buf, 2);

        if s.comp_on {
            self.comp.configure(&s.comp);
            self.comp.process(buf, 2);
            self.meters.comp_gr_db.set(self.comp.gain_reduction_db());
        } else {
            if self.comp_was_on {
                self.comp = Compressor::new();
            }
            self.meters.comp_gr_db.set(0.0);
        }
        self.comp_was_on = s.comp_on;

        self.channels.configure(s.mono, s.balance, s.width);
        self.channels.process(buf);

        self.boost.set_db(s.boost_db);
        self.boost.process(buf, 2);
    }

    /// Ajoute le soundboard puis applique le limiteur (ou la troncature) de sortie.
    pub fn finish(&mut self, buf: &mut [f32], s: &Settings) {
        self.player.mix_into(buf, db_to_lin(s.soundboard_db));
        let clipped = self.limiter.process(buf, s.limiter_on);
        self.meters.clip.store(clipped, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{peak_dbfs, sine};

    #[test]
    fn default_settings_keep_a_quiet_sine() {
        let mut p = Processor::new(Arc::new(Meters::default()));
        let mut s = Settings::default();
        s.denoise_on = false;
        let mut buf = sine(1000.0, -20.0, 9600, 2);
        p.process_voice(&mut buf, &s);
        p.finish(&mut buf, &s);
        assert!((peak_dbfs(&buf[9600..]) + 20.0).abs() < 0.1);
    }

    #[test]
    fn boost_then_limiter_stays_under_ceiling() {
        let mut p = Processor::new(Arc::new(Meters::default()));
        let mut s = Settings::default();
        s.denoise_on = false;
        s.boost_db = 20.0;
        let mut buf = sine(1000.0, -6.0, 9600, 2);
        p.process_voice(&mut buf, &s);
        p.finish(&mut buf, &s);
        assert!(buf.iter().all(|v| v.abs() <= 0.891));
    }

    #[test]
    fn clip_flag_set_when_limiter_off() {
        let meters = Arc::new(Meters::default());
        let mut p = Processor::new(meters.clone());
        let mut s = Settings::default();
        s.denoise_on = false;
        s.limiter_on = false;
        s.boost_db = 20.0;
        let mut buf = sine(1000.0, -6.0, 4800, 2);
        p.process_voice(&mut buf, &s);
        p.finish(&mut buf, &s);
        assert!(meters.clip.load(Ordering::Relaxed));
        assert!(buf.iter().all(|v| v.abs() <= 1.0));
    }

    #[test]
    fn soundboard_is_mixed_after_the_voice() {
        let mut p = Processor::new(Arc::new(Meters::default()));
        let mut s = Settings::default();
        s.denoise_on = false;
        p.apply_pad_command(PadCommand::Play { pad: 0, data: vec![0.3f32; 200].into(), gain: 1.0 });
        let mut buf = vec![0.1f32; 2 * 200];
        p.process_voice(&mut buf, &s);
        p.finish(&mut buf, &s);
        // Le limiteur retarde la sortie : on lit apres son retard.
        let i = 2 * crate::limiter::DELAY_FRAMES;
        assert!((buf[i] - 0.4).abs() < 1e-3, "{}", buf[i]);
        assert!((buf[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn gain_reduction_meter_follows_compressor() {
        let meters = Arc::new(Meters::default());
        let mut p = Processor::new(meters.clone());
        let mut s = Settings::default();
        s.denoise_on = false;
        s.comp_on = true;
        let mut buf = sine(1000.0, -6.0, 24_000, 2);
        p.process_voice(&mut buf, &s);
        assert!(meters.comp_gr_db.get() > 3.0);
        s.comp_on = false;
        let mut buf = sine(1000.0, -6.0, 480, 2);
        p.process_voice(&mut buf, &s);
        assert_eq!(meters.comp_gr_db.get(), 0.0);
    }
}
