//! Decodage des fichiers du soundboard (WAV, MP3, OGG, FLAC) vers du f32 stereo entrelace a 48 kHz.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};

use clearmic_dsp::player::PadCommand;

use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::devices::REQUIRED_RATE;

/// Garde-fou memoire (pas une limite de duree) : un son decode occupe 384 Ko par seconde, ce
/// plafond de 1 Go correspond a environ 45 minutes par fichier.
pub const MAX_DECODED_BYTES: u64 = 1 << 30;

fn too_big(frames: u64, rate: u64) -> Option<String> {
    let bytes = frames * 2 * 4 * 48_000 / rate.max(1);
    (bytes > MAX_DECODED_BYTES).then(|| "fichier trop volumineux en mémoire (plus de 45 minutes)".to_string())
}

/// Sons prets a jouer, partages avec le thread des raccourcis clavier (qui declenche les pads
/// sans passer par l'interface, meme fenetre reduite).
#[derive(Default)]
pub struct Bank(Mutex<HashMap<usize, Ready>>);

/// Son decode et gain lineaire.
type Ready = (Arc<[f32]>, f32);

impl Bank {
    /// Remplace le contenu : (identifiant du pad, son, gain lineaire).
    pub fn replace(&self, entries: Vec<(usize, Arc<[f32]>, f32)>) {
        if let Ok(mut g) = self.0.lock() {
            *g = entries.into_iter().map(|(id, d, gain)| (id, (d, gain))).collect();
        }
    }

    pub fn play_command(&self, pad: usize) -> Option<PadCommand> {
        let g = self.0.lock().ok()?;
        let (data, gain) = g.get(&pad)?;
        Some(PadCommand::Play { pad, data: data.clone(), gain: *gain })
    }
}

/// Decode `path`. Le message d'erreur nomme la raison du refus.
pub fn decode_file(path: &Path) -> Result<Arc<[f32]>, String> {
    let file = File::open(path).map_err(|e| format!("lecture impossible ({e})"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|_| "format non reconnu ou fichier corrompu".to_string())?;

    let track = format.default_track(TrackType::Audio).ok_or("aucune piste audio")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("aucune piste audio")?;
    let declared_rate = params.sample_rate.unwrap_or(REQUIRED_RATE) as u64;
    if let Some(frames) = track.num_frames
        && let Some(reason) = too_big(frames, declared_rate)
    {
        return Err(reason);
    }
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|_| "codec non pris en charge".to_string())?;

    let mut stereo: Vec<f32> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    let mut rate = declared_rate as u32;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::IoError(_)) => break,
            Err(_) => return Err("fichier corrompu".into()),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let channels = buf.spec().channels().count();
                rate = buf.spec().rate();
                if channels == 0 || rate == 0 {
                    return Err("format audio invalide".into());
                }
                scratch.resize(buf.samples_interleaved(), f32::MID);
                buf.copy_to_slice_interleaved(&mut scratch);
                for fr in scratch.chunks_exact(channels) {
                    stereo.push(fr[0]);
                    stereo.push(if channels > 1 { fr[1] } else { fr[0] });
                }
                if let Some(reason) = too_big((stereo.len() / 2) as u64, rate as u64) {
                    return Err(reason);
                }
            }
            Err(Error::DecodeError(_)) => {}
            Err(_) => return Err("fichier corrompu".into()),
        }
    }
    if stereo.is_empty() {
        return Err("aucun echantillon audio decode".into());
    }
    let out = if rate == REQUIRED_RATE { stereo } else { resample(&stereo, rate, REQUIRED_RATE) };
    Ok(out.into())
}

/// Reechantillonnage par interpolation cubique (Catmull-Rom) d'un signal stereo entrelace.
pub fn resample(src: &[f32], from: u32, to: u32) -> Vec<f32> {
    let frames = src.len() / 2;
    let out_frames = (frames as u64 * to as u64 / from as u64) as usize;
    let step = from as f64 / to as f64;
    let at = |i: isize, ch: usize| -> f32 {
        let i = i.clamp(0, frames as isize - 1) as usize;
        src[2 * i + ch]
    };
    let mut out = Vec::with_capacity(out_frames * 2);
    for n in 0..out_frames {
        let pos = n as f64 * step;
        let i = pos.floor() as isize;
        let t = (pos - i as f64) as f32;
        for ch in 0..2 {
            let (p0, p1, p2, p3) = (at(i - 1, ch), at(i, ch), at(i + 1, ch), at(i + 2, ch));
            let v = 0.5
                * ((2.0 * p1)
                    + (-p0 + p2) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t);
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// WAV PCM 16 bits minimal.
    fn wav_bytes(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        v.extend_from_slice(&(channels * 2).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("clearmic-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::File::create(&path).unwrap().write_all(bytes).unwrap();
        path
    }

    #[test]
    fn decodes_mono_wav_to_stereo_48k() {
        let samples: Vec<i16> = (0..4800).map(|i| (i % 100 * 100) as i16).collect();
        let p = temp_file("mono.wav", &wav_bytes(48_000, 1, &samples));
        let data = decode_file(&p).unwrap();
        assert_eq!(data.len(), 4800 * 2);
        assert_eq!(data[200], data[201], "mono duplique sur L et R");
    }

    #[test]
    fn resamples_44100_to_48000() {
        let samples: Vec<i16> = vec![1000; 44_100];
        let p = temp_file("rate44.wav", &wav_bytes(44_100, 1, &samples));
        let data = decode_file(&p).unwrap();
        let frames = data.len() / 2;
        assert!((frames as i64 - 48_000).abs() <= 1, "{frames} trames");
        assert!((data[48_000] - 1000.0 / 32768.0).abs() < 1e-3);
    }

    #[test]
    fn long_files_are_accepted() {
        // Plus de 30 s : l'ancienne limite de duree n'existe plus.
        let samples = vec![0i16; 8000 * 95];
        let p = temp_file("long.wav", &wav_bytes(8000, 1, &samples));
        let data = decode_file(&p).unwrap();
        assert!((data.len() as i64 / 2 - 48_000 * 95).abs() <= 2);
    }

    #[test]
    fn memory_guard_applies_to_huge_files() {
        assert!(too_big(48_000 * 60 * 60, 48_000).is_some());
        assert!(too_big(48_000 * 60 * 40, 48_000).is_none());
        assert!(too_big(44_100 * 60 * 60, 44_100).is_some());
    }

    #[test]
    fn bank_builds_play_commands() {
        let bank = Bank::default();
        bank.replace(vec![(3, vec![0.5f32; 4].into(), 0.25)]);
        match bank.play_command(3) {
            Some(PadCommand::Play { pad, gain, data }) => {
                assert_eq!((pad, gain, data.len()), (3, 0.25, 4));
            }
            _ => panic!("commande attendue"),
        }
        assert!(bank.play_command(9).is_none());
    }

    #[test]
    fn rejects_garbage() {
        let p = temp_file("garbage.mp3", b"ceci n'est pas de l'audio du tout, loin de la");
        assert!(decode_file(&p).is_err());
    }

    #[test]
    fn rejects_missing_file() {
        let err = decode_file(Path::new("Z:/nexiste/pas.wav")).unwrap_err();
        assert!(err.contains("lecture impossible"), "{err}");
    }
}
