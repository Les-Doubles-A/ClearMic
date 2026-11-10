//! Sortie WASAPI en mode partage a faible latence (IAudioClient3, periode minimale du moteur).
//!
//! cpal impose la periode par defaut du moteur (10 ms). IAudioClient3 permet de descendre a la
//! periode minimale annoncee par le pilote (2,67 ms pour le « CABLE Input » de VB-CABLE). Si le
//! peripherique ne propose rien de plus court, ou si l'ouverture echoue, `open` rend le rappel a
//! l'appelant qui utilise alors le flux cpal habituel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    IAudioClient3, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject};
use windows::core::{GUID, PCWSTR};

/// Source du son : remplit un bloc stereo entrelace (le nombre de trames varie d'un appel a
/// l'autre). Rendue a l'appelant si l'ouverture echoue.
pub trait Render: Send + 'static {
    fn render(&mut self, out: &mut [f32]);
}
/// Appele une fois si le flux s'arrete de lui-meme : (peripherique perdu, message).
pub type OnFailure = Box<dyn Fn(bool, String) + Send>;

const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const WAIT_TIMEOUT_MS: u32 = 50;

pub struct LowLatencyOutput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Periode obtenue, en trames.
    pub period_frames: u32,
}

impl Drop for LowLatencyOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Echec d'ouverture : la source est rendue pour un repli sur cpal.
pub struct OpenError<R> {
    pub message: String,
    pub render: Option<R>,
}

struct Opened {
    client: IAudioClient3,
    render: IAudioRenderClient,
    event: HANDLE,
    buffer_frames: u32,
    channels: usize,
    period_frames: u32,
}

/// Ouvre le point de terminaison `endpoint_id` (identifiant WASAPI, `cpal::Device::id`).
pub fn open<R: Render>(endpoint_id: &str, render: R, on_failure: OnFailure) -> Result<LowLatencyOutput, OpenError<R>> {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Result<u32, OpenError<R>>>();
    let id = endpoint_id.to_string();
    let thread_stop = stop.clone();
    let thread = std::thread::Builder::new()
        .name("clearmic-lowlat".into())
        .spawn(move || run(id, render, thread_stop, tx, on_failure));
    let Ok(thread) = thread else {
        // La source a ete deplacee dans la fermeture qui n'a pas pu demarrer : elle est perdue.
        return Err(OpenError { message: "thread impossible a creer".into(), render: None });
    };
    match rx.recv() {
        Ok(Ok(period_frames)) => Ok(LowLatencyOutput { stop, thread: Some(thread), period_frames }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(OpenError { message: "thread interrompu".into(), render: None }),
    }
}

fn run<R: Render>(
    id: String,
    mut render: R,
    stop: Arc<AtomicBool>,
    report: mpsc::Sender<Result<u32, OpenError<R>>>,
    on_failure: OnFailure,
) {
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        match setup(&id) {
            Err(message) => {
                let _ = report.send(Err(OpenError { message, render: Some(render) }));
            }
            Ok(o) => {
                let mut task = 0u32;
                let mmcss = AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &mut task).ok();
                let _ = report.send(Ok(o.period_frames));
                drop(report);
                if let Err((lost, message)) = pump(&o, &mut render, &stop) {
                    on_failure(lost, message);
                }
                let _ = o.client.Stop();
                let _ = CloseHandle(o.event);
                if let Some(h) = mmcss {
                    let _ = AvRevertMmThreadCharacteristics(h);
                }
            }
        }
        if com {
            CoUninitialize();
        }
    }
}

unsafe fn setup(id: &str) -> Result<Opened, String> {
    unsafe {
        let err = |what: &str, e: windows::core::Error| format!("{what} : {e}");
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| err("enumerateur", e))?;
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        let dev = en.GetDevice(PCWSTR(wide.as_ptr())).map_err(|e| err("peripherique", e))?;
        let client: IAudioClient3 = dev.Activate(CLSCTX_ALL, None).map_err(|e| err("IAudioClient3", e))?;
        let fmt = client.GetMixFormat().map_err(|e| err("format", e))?;
        let result = setup_with_format(&client, fmt);
        let channels = (*fmt).nChannels as usize;
        CoTaskMemFree(Some(fmt as *const _));
        let (period, event) = result?;
        let render: IAudioRenderClient = client.GetService().map_err(|e| err("service de rendu", e))?;
        let buffer_frames = client.GetBufferSize().map_err(|e| err("taille du tampon", e))?;
        // Tampon initial en silence : le premier evenement arrive quand le moteur en a besoin.
        render.GetBuffer(buffer_frames).map_err(|e| err("tampon initial", e))?;
        render
            .ReleaseBuffer(buffer_frames, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)
            .map_err(|e| err("tampon initial", e))?;
        client.Start().map_err(|e| err("demarrage", e))?;
        Ok(Opened { client, render, event, buffer_frames, channels, period_frames: period })
    }
}

/// Verifie le format du moteur, choisit la periode minimale et initialise le flux.
unsafe fn setup_with_format(client: &IAudioClient3, fmt: *const WAVEFORMATEX) -> Result<(u32, HANDLE), String> {
    unsafe {
        let tag = (*fmt).wFormatTag;
        let bits = (*fmt).wBitsPerSample;
        let rate = (*fmt).nSamplesPerSec;
        let channels = (*fmt).nChannels;
        let float = tag == WAVE_FORMAT_IEEE_FLOAT
            || (tag == WAVE_FORMAT_EXTENSIBLE && {
                let ext = std::ptr::read_unaligned(fmt as *const WAVEFORMATEXTENSIBLE);
                let sub = ext.SubFormat;
                sub == SUBTYPE_IEEE_FLOAT
            });
        if !float || bits != 32 || rate != 48_000 || channels == 0 {
            return Err(format!("format du moteur non pris en charge ({rate} Hz, {channels} canaux, {bits} bits)"));
        }
        let (mut default, mut fundamental, mut min, mut max) = (0u32, 0u32, 0u32, 0u32);
        client
            .GetSharedModeEnginePeriod(fmt, &mut default, &mut fundamental, &mut min, &mut max)
            .map_err(|e| format!("periodes du moteur : {e}"))?;
        if min == 0 || min >= default {
            return Err(format!("aucune periode plus courte que {default} trames"));
        }
        client
            .InitializeSharedAudioStream(AUDCLNT_STREAMFLAGS_EVENTCALLBACK, min, fmt, None)
            .map_err(|e| format!("initialisation du flux : {e}"))?;
        let event = CreateEventW(None, false, false, None).map_err(|e| format!("evenement : {e}"))?;
        client.SetEventHandle(event).map_err(|e| format!("evenement : {e}"))?;
        Ok((min, event))
    }
}

unsafe fn pump<R: Render>(o: &Opened, render: &mut R, stop: &AtomicBool) -> Result<(), (bool, String)> {
    unsafe {
        let fail = |e: windows::core::Error| (e.code() == AUDCLNT_E_DEVICE_INVALIDATED, e.to_string());
        let mut scratch = vec![0f32; 2 * o.buffer_frames as usize];
        while !stop.load(Ordering::Acquire) {
            if WaitForSingleObject(o.event, WAIT_TIMEOUT_MS) != WAIT_OBJECT_0 {
                continue;
            }
            let padding = o.client.GetCurrentPadding().map_err(fail)?;
            let frames = o.buffer_frames.saturating_sub(padding) as usize;
            if frames == 0 {
                continue;
            }
            let ptr = o.render.GetBuffer(frames as u32).map_err(fail)? as *mut f32;
            let out = std::slice::from_raw_parts_mut(ptr, frames * o.channels);
            let block = &mut scratch[..frames * 2];
            render.render(block);
            if o.channels == 2 {
                out.copy_from_slice(block);
            } else if o.channels == 1 {
                for (d, f) in out.iter_mut().zip(block.as_chunks::<2>().0) {
                    *d = 0.5 * (f[0] + f[1]);
                }
            } else {
                for (d, f) in out.chunks_exact_mut(o.channels).zip(block.as_chunks::<2>().0) {
                    d.fill(0.0);
                    d[0] = f[0];
                    d[1] = f[1];
                }
            }
            o.render.ReleaseBuffer(frames as u32, 0).map_err(fail)?;
        }
        Ok(())
    }
}

/// Source muette qui compte les appels (diagnostic `--probe-lowlat`).
struct Silence {
    calls: Arc<std::sync::atomic::AtomicU64>,
    frames: Arc<std::sync::atomic::AtomicU64>,
}

impl Render for Silence {
    fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.frames.fetch_add((out.len() / 2) as u64, Ordering::Relaxed);
    }
}

/// Diagnostic : ouvre la sortie en silence pendant `seconds` et affiche la cadence des rappels.
pub fn probe(endpoint_id: &str, seconds: u64) {
    use std::sync::atomic::AtomicU64;
    let calls = Arc::new(AtomicU64::new(0));
    let frames = Arc::new(AtomicU64::new(0));
    let source = Silence { calls: calls.clone(), frames: frames.clone() };
    match open(endpoint_id, source, Box::new(|lost, m| println!("flux arrete (perdu : {lost}) : {m}"))) {
        Ok(out) => {
            println!("flux ouvert, periode {} trames ({:.2} ms)", out.period_frames, out.period_frames as f32 / 48.0);
            std::thread::sleep(Duration::from_secs(seconds));
            let (c, f) = (calls.load(Ordering::Relaxed), frames.load(Ordering::Relaxed));
            println!("{c} rappels en {seconds} s, {:.1} trames par rappel, {:.0} Hz effectif", f as f64 / c.max(1) as f64, f as f64 / seconds as f64);
        }
        Err(e) => println!("ouverture impossible : {}", e.message),
    }
}
