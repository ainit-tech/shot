//! The microphone during a recording (decision 17). It is one switch on the panel, which works at
//! any moment of the recording. Its sound goes into a file of its own on a thread of its own, in
//! place by the time of the recording, so that a problem with it can't hurt the picture, and a
//! recording during which the microphone stayed off has no sound at all. It joins the video when
//! the recording ends.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, eCapture, eConsole,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

use super::video::VOICE_RATE;

/// The name of the microphone's file next to the recording.
pub const FILE: &str = "voice.pcm";
/// How long a buffer of sound Windows keeps for the thread, in Media Foundation's units, and how
/// often the thread looks into it.
const BUFFER: i64 = 2_000_000;
const LOOK: Duration = Duration::from_millis(10);
/// How far the sound may stray from the time of the recording before it is put right, in samples:
/// putting it right is heard, and this much isn't seen.
const LEEWAY: u64 = VOICE_RATE as u64 / 20;
/// The quietest sound the panel shows, in decibels.
const QUIETEST: f32 = -50.0;

/// What the panel and the microphone's thread say to each other.
#[derive(Default)]
pub struct Line {
    /// Whether the user wants the microphone on.
    pub wanted: AtomicBool,
    /// How loud it is now, from 0 to 1000.
    pub level: AtomicU32,
    /// Why the microphone can't be had.
    pub problem: Mutex<Option<String>>,
}

/// How loud some sound is for the eye, from 0 to 1000: by decibels, as loudness is heard.
fn loudness(samples: &[i16]) -> u32 {
    let peak = samples.iter().map(|&sample| (sample as i32).abs()).max().unwrap_or(0);
    if peak == 0 {
        return 0;
    }
    let decibels = 20.0 * (peak as f32 / 32768.0).log10();
    ((1.0 - decibels / QUIETEST).clamp(0.0, 1.0) * 1000.0) as u32
}

/// Sound going into a file, by its place in the recording. Samples follow each other, while the
/// clock of the sound and that of the recording slowly part, and the microphone may be off for a
/// while: a gap becomes silence, and what would come too late loses its beginning.
struct Track<W: Write> {
    out: W,
    written: u64,
}

impl<W: Write> Track<W> {
    fn new(out: W) -> Self {
        Track { out, written: 0 }
    }

    /// Puts samples at their place, counted in samples from the start of the recording.
    fn put(&mut self, place: u64, samples: &[i16]) -> io::Result<()> {
        if place > self.written + LEEWAY {
            const SILENCE: [u8; 4096] = [0; 4096];
            let mut left = (place - self.written) * 2;
            while left > 0 {
                let part = left.min(SILENCE.len() as u64) as usize;
                self.out.write_all(&SILENCE[..part])?;
                left -= part as u64;
            }
            self.written = place;
        }
        let late = if self.written > place + LEEWAY { (self.written - place) as usize } else { 0 };
        let samples = &samples[late.min(samples.len())..];
        let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        self.out.write_all(&bytes)?;
        self.written += samples.len() as u64;
        Ok(())
    }
}

/// The default microphone of Windows, open.
struct Microphone {
    client: IAudioClient,
    capture: IAudioCaptureClient,
}

impl Microphone {
    fn open() -> windows_core::Result<Microphone> {
        // A test never touches the real microphone: it makes the antivirus raise an alarm.
        if cfg!(test) {
            return Err(windows_core::Error::empty());
        }
        unsafe {
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = devices.GetDefaultAudioEndpoint(eCapture, eConsole)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            // Windows converts whatever the microphone gives into this.
            let format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 1,
                nSamplesPerSec: VOICE_RATE,
                nAvgBytesPerSec: VOICE_RATE * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER, 0, &format, None)?;
            let capture: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            Ok(Microphone { client, capture })
        }
    }

    /// Takes what has come since the last time.
    fn read(&mut self, samples: &mut Vec<i16>) -> windows_core::Result<()> {
        samples.clear();
        unsafe {
            while self.capture.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (null_mut(), 0u32, 0u32);
                self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let start = samples.len();
                samples.resize(start + frames as usize, 0);
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 == 0 {
                    std::ptr::copy_nonoverlapping(data.cast::<u8>(), samples[start..].as_mut_ptr().cast::<u8>(), frames as usize * 2);
                }
                self.capture.ReleaseBuffer(frames)?;
            }
        }
        Ok(())
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

/// The microphone's thread, for as long as a recording goes on: it opens the microphone when the
/// user wants it and lets it go when not, and says whether any sound has gone into the file.
/// `now` is the time of the recording, nothing while it isn't going; the microphone shows its
/// level meanwhile, and nothing is kept.
pub fn listen(line: &Line, ended: impl Fn() -> bool, now: impl Fn() -> Option<Duration>, path: &Path) -> bool {
    let (mut microphone, mut track) = (None::<Microphone>, None::<Track<BufWriter<File>>>);
    let mut samples = Vec::new();
    let fail = |problem: String| {
        line.wanted.store(false, Ordering::Relaxed);
        line.level.store(0, Ordering::Relaxed);
        if let Ok(mut told) = line.problem.lock() {
            *told = Some(problem);
        }
    };
    while !ended() {
        let wanted = line.wanted.load(Ordering::Relaxed);
        if wanted && microphone.is_none() {
            match Microphone::open() {
                Ok(open) => microphone = Some(open),
                Err(error) => fail(error.message()),
            }
        } else if !wanted && microphone.is_some() {
            microphone = None;
            line.level.store(0, Ordering::Relaxed);
        }
        if let Some(open) = &mut microphone {
            if let Err(error) = open.read(&mut samples) {
                microphone = None;
                fail(error.message());
            } else if !samples.is_empty() {
                line.level.store(loudness(&samples), Ordering::Relaxed);
                if let Some(time) = now() {
                    let end = (time.as_secs_f64() * VOICE_RATE as f64) as u64;
                    let place = end.saturating_sub(samples.len() as u64);
                    let written = match &mut track {
                        Some(track) => track.put(place, &samples),
                        None => File::create(path).and_then(|file| {
                            let mut new = Track::new(BufWriter::new(file));
                            new.put(place, &samples)?;
                            track = Some(new);
                            Ok(())
                        }),
                    };
                    if let Err(error) = written {
                        microphone = None;
                        fail(error.to_string());
                    }
                }
            }
        }
        thread::sleep(LOOK);
    }
    drop(microphone);
    let finished = track.is_some_and(|mut track| track.written > 0 && track.out.flush().is_ok());
    if !finished {
        let _ = std::fs::remove_file(path);
    }
    finished
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_of(steps: &[(u64, usize)]) -> Vec<u8> {
        let mut track = Track::new(Vec::new());
        for &(place, count) in steps {
            track.put(place, &vec![1i16; count]).unwrap();
        }
        track.out
    }

    #[test]
    fn sound_stays_in_place() {
        // Samples following each other, a little off, follow each other.
        assert_eq!(track_of(&[(0, 480), (500, 480)]).len(), 960 * 2);
        // A gap becomes silence.
        let file = track_of(&[(0, 100), (48_000, 100)]);
        assert_eq!(file.len(), (48_000 + 100) * 2);
        assert_eq!(&file[..2], &[1, 0]);
        assert_eq!(&file[100 * 2..100 * 2 + 2], &[0, 0]);
        assert_eq!(&file[48_000 * 2..48_000 * 2 + 2], &[1, 0]);
        // What comes too late loses what would overlap.
        let file = track_of(&[(0, 10_000), (1_000, 10_000)]);
        assert_eq!(file.len(), 11_000 * 2);
    }

    #[test]
    fn loudness_goes_by_decibels() {
        assert_eq!(loudness(&[]), 0);
        assert_eq!(loudness(&[0, 0]), 0);
        assert!(loudness(&[32767]) >= 999);
        let (quiet, speech) = (loudness(&[300]), loudness(&[3000]));
        assert!(0 < quiet && quiet < speech && speech < 1000, "{quiet} {speech}");
    }

    /// The thread lives and ends without a microphone wanted, and leaves nothing.
    #[test]
    fn with_the_microphone_off_nothing_is_written() {
        let path = std::env::temp_dir().join(format!("ainitshot-voice-{}.pcm", std::process::id()));
        let line = Line::default();
        let looks = std::sync::atomic::AtomicU32::new(0);
        let ended = || looks.fetch_add(1, Ordering::Relaxed) >= 3;
        assert!(!listen(&line, ended, || Some(Duration::from_secs(1)), &path));
        assert!(!path.exists());
    }

    /// In a test the microphone is never opened: what is wanted comes back as a problem.
    #[test]
    fn a_test_never_opens_the_microphone() {
        let path = std::env::temp_dir().join(format!("ainitshot-voice-{}-2.pcm", std::process::id()));
        let line = Line::default();
        line.wanted.store(true, Ordering::Relaxed);
        let looks = std::sync::atomic::AtomicU32::new(0);
        let ended = || looks.fetch_add(1, Ordering::Relaxed) >= 2;
        assert!(!listen(&line, ended, || Some(Duration::from_secs(1)), &path));
        assert!(!line.wanted.load(Ordering::Relaxed));
        assert!(line.problem.lock().unwrap().is_some());
    }
}
