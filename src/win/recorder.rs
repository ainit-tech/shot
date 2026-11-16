//! A recording runs in a process of its own (decision 17): the program itself, started with
//! `--recording`. That process reads the screen and packs the video, at a priority above the
//! programs that only keep the processor busy, and the graphics card's encoder there can't take
//! the program down with it: NVIDIA's has been seen to crash the process now and then as it is
//! let go, so that process ends without letting go of anything once its file is written. The
//! program keeps the windows of the recording, its time and the microphone, and says to the
//! process when the recording begins, pauses, goes on and stops; the process answers with the
//! time recorded and, at the end, with what it has made.

use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows_sys::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW, GetCurrentProcess, SetPriorityClass, TerminateProcess,
};
use windows_sys::core::PCWSTR;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::w;

use super::encoder::{self, CardWriter};
use super::grab::{Grab, Picture};
use super::record::Shared;
use super::video::{FRAMES, SECOND, Writer};
use crate::geometry::Rect;
use crate::text::tr;

/// What the program is started with to be the process of a recording.
pub const ARGUMENT: &str = "--recording";
/// How long a recording that has been told to stop may take to end its file.
const ENDING: Duration = Duration::from_secs(60);

/// What a recording is to be: the area of the desktop, the size of its video, the scale of the
/// screen, where its panel stands, its file, whether the graphics card packs it and its frames
/// a second.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Order {
    pub area: [i32; 4],
    pub size: (u32, u32),
    pub scale: f32,
    pub panel: [i32; 4],
    pub path: PathBuf,
    pub card: bool,
    pub frames: u32,
}

/// What a recording has made: its frames, its length in seconds, how it was read and packed, and
/// its file.
#[derive(Debug, Serialize, Deserialize)]
pub struct Made {
    pub frames: u64,
    pub seconds: f64,
    pub way: String,
    pub path: PathBuf,
}

/// A recording, from the end of the count until it is stopped: in a process of its own if it can
/// be started, here otherwise, on the processor alone. Nothing, if it was stopped before it began.
pub(super) fn capture(shared: &Shared, order: &Order) -> Result<Option<Made>, String> {
    match Apart::start(order) {
        Some(apart) => apart.follow(shared),
        None => record(shared, &Order { card: false, ..order.clone() }, |_| {}),
    }
}

fn rect([left, top, right, bottom]: [i32; 4]) -> Rect {
    Rect::new(left, top, right, bottom)
}

/// The recording's process, seen from the program.
struct Apart {
    child: Child,
    input: ChildStdin,
    /// What it says, line by line.
    said: Receiver<String>,
}

impl Apart {
    fn start(order: &Order) -> Option<Apart> {
        let mut child = Command::new(program()?)
            .arg(ARGUMENT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .inspect_err(|problem| eprintln!("The recording's process didn't start: {problem}"))
            .ok()?;
        let (mut input, output) = (child.stdin.take()?, child.stdout.take()?);
        writeln!(input, "{}", serde_json::to_string(order).ok()?).and_then(|()| input.flush()).ok()?;
        let (tell, said) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if tell.send(line).is_err() {
                    break;
                }
            }
        });
        Some(Apart { child, input, said })
    }

    /// Tells the process what the program's clock does, and keeps the time it has recorded, until
    /// it has ended.
    fn follow(mut self, shared: &Shared) -> Result<Option<Made>, String> {
        let (mut begun, mut paused, mut stopping) = (false, false, None);
        loop {
            let (now_begun, now_paused) = {
                let clock = shared.clock();
                (clock.begun.is_some(), clock.paused.is_some())
            };
            let mut word = None;
            if shared.stopped.load(Ordering::Relaxed) {
                if stopping.is_none() {
                    stopping = Some(Instant::now());
                    word = Some("stop");
                }
            } else if now_begun && !begun {
                begun = true;
                word = Some("go");
            } else if begun && now_paused != paused {
                paused = now_paused;
                word = Some(if paused { "pause" } else { "resume" });
            }
            if let Some(word) = word {
                // A process that has gone no longer listens; what it said last tells why.
                let _ = writeln!(self.input, "{word}").and_then(|()| self.input.flush());
            }
            loop {
                match self.said.try_recv() {
                    Ok(line) => {
                        let (what, rest) = line.split_once(' ').unwrap_or((&line, ""));
                        match what {
                            "time" => shared.time.store(rest.parse().unwrap_or(0), Ordering::Relaxed),
                            "done" => return self.end(serde_json::from_str(rest).map(Some).map_err(|problem| problem.to_string())),
                            "nothing" => return self.end(Ok(None)),
                            "failed" => return self.end(Err(rest.to_string())),
                            _ => {}
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        let code = self.child.wait().ok().and_then(|status| status.code()).unwrap_or(-1);
                        return Err(format!("{} ({code:#x})", tr("The recording's process ended unexpectedly")));
                    }
                }
            }
            if stopping.is_some_and(|since| since.elapsed() > ENDING) {
                let _ = self.child.kill();
                return Err(tr("The recording's process ended unexpectedly").into());
            }
            thread::sleep(Duration::from_millis(4));
        }
    }

    fn end(mut self, outcome: Result<Option<Made>, String>) -> Result<Option<Made>, String> {
        let _ = self.child.wait();
        outcome
    }
}

/// The program to start for a recording: this one. A test has none of its own, but may be given
/// the program built.
fn program() -> Option<PathBuf> {
    if cfg!(test) {
        return std::env::var_os("RECORDING_PROGRAM").map(PathBuf::from);
    }
    std::env::current_exe().ok()
}

/// The process of a recording, from its start to its end: the order comes first, then what the
/// program's clock does.
pub fn serve() -> ! {
    unsafe {
        // A crash ends the process quietly: the program says that the recording stopped.
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
        SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS);
        // The mouse and the screen in real pixels, as the program has them.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();
    }
    on_time();
    serve_on(BufReader::new(std::io::stdin()), std::io::stdout());
    // Nothing is let go: the file is written, and the card's encoder might crash as it went.
    unsafe { TerminateProcess(GetCurrentProcess(), 0) };
    unreachable!()
}

/// Asks Windows' scheduler for sound and video to wake this thread on time, however busy the
/// processor is. Its library is looked for: a program that links to it wouldn't start where it
/// is missing.
fn on_time() {
    unsafe {
        let library = LoadLibraryExW(w!("avrt.dll"), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if library.is_null() {
            return;
        }
        let Some(found) = GetProcAddress(library, c"AvSetMmThreadCharacteristicsW".as_ptr().cast()) else { return };
        let set: unsafe extern "system" fn(PCWSTR, *mut u32) -> HANDLE = std::mem::transmute(found);
        let mut task = 0;
        set(w!("Capture"), &mut task);
    }
}

/// A recording's process at work, from `input` and saying what it has to say to `output`.
fn serve_on(input: impl BufRead + Send + 'static, mut output: impl Write) {
    let mut lines = input.lines();
    let order = lines.next().and_then(Result::ok).and_then(|line| serde_json::from_str::<Order>(&line).ok());
    let Some(order) = order else {
        let _ = writeln!(output, "failed {}", tr("The recording didn't start"));
        return;
    };
    let shared = Arc::new(Shared::default());
    let listening = shared.clone();
    thread::spawn(move || {
        for line in lines {
            let Ok(line) = line else { break };
            let mut clock = listening.clock();
            match line.trim() {
                "go" => clock.begun = Some(Instant::now()),
                "pause" => clock.paused = Some(Instant::now()),
                "resume" => {
                    if let Some(since) = clock.paused.take() {
                        clock.left_out += since.elapsed();
                    }
                }
                "stop" => break,
                _ => {}
            }
        }
        // Stopped, or the program has gone.
        listening.stopped.store(true, Ordering::Relaxed);
    });
    let outcome = record(&shared, &order, |time| {
        let _ = writeln!(output, "time {}", time.as_millis()).and_then(|()| output.flush());
    });
    let _ = match outcome {
        Ok(Some(made)) => writeln!(output, "done {}", serde_json::to_string(&made).unwrap_or_default()),
        Ok(None) => writeln!(output, "nothing"),
        Err(problem) => writeln!(output, "failed {}", problem.replace('\n', " ")),
    };
    let _ = output.flush();
}

/// What packs the frames: the graphics card's encoder, or the processor's.
enum Packer {
    Card(CardWriter),
    Processor(Writer),
}

impl Packer {
    /// Adds a picture to the file, and says whether it went in.
    fn write(&mut self, picture: Picture, time: i64, length: i64) -> windows_core::Result<bool> {
        match (self, picture) {
            (Packer::Card(writer), Picture::Card(texture)) => writer.write(texture, time, length),
            (Packer::Card(writer), Picture::Memory(frame)) => writer.write_pixels(frame.pixels, frame.width as usize * 4, time, length),
            (Packer::Processor(writer), Picture::Memory(frame)) => writer.write(frame.pixels, frame.width as usize * 4, time, length).map(|()| true),
            // The pictures stay on the card only for its own encoder.
            (Packer::Processor(_), Picture::Card(_)) => Err(windows_core::Error::empty()),
        }
    }

    fn finish(self) -> windows_core::Result<()> {
        match self {
            Packer::Card(writer) => writer.finish(),
            Packer::Processor(writer) => writer.finish(),
        }
    }

    fn name(&self) -> String {
        match self {
            Packer::Card(writer) => writer.encoder().to_string(),
            Packer::Processor(_) => "the processor".into(),
        }
    }
}

/// The graphics card's way, if the order asks for it and the card can, or the processor's, with
/// the frames a second it takes, the processor no more than it keeps up with, and the file. A
/// card that couldn't keeps the file of the order open, so the processor writes another beside it.
fn packing(order: &Order) -> Result<(Grab, Packer, u32, PathBuf), String> {
    let (area, panel) = (rect(order.area), rect(order.panel));
    let mut path = order.path.clone();
    if order.card {
        let card = || -> Result<(Grab, Packer, u32, PathBuf), String> {
            let grab = Grab::new(area, order.size, order.scale, panel, true).map_err(|problem| problem.to_string())?;
            let device = match grab.device() {
                Some(device) => device,
                None => encoder::video_device(None).map_err(|problem| problem.to_string())?.0,
            };
            let writer =
                CardWriter::create(&order.path, &device, grab.size(), order.size, order.frames).map_err(|problem| problem.to_string())?;
            Ok((grab, Packer::Card(writer), order.frames, order.path.clone()))
        };
        match card() {
            Ok(card) => return Ok(card),
            Err(problem) => eprintln!("The graphics card doesn't record: {problem}"),
        }
        let name = order.path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        path = order.path.with_file_name(format!("{name}-processor.mp4"));
    }
    let grab = Grab::new(area, order.size, order.scale, panel, false).map_err(|problem| problem.to_string())?;
    let rate = order.frames.min(FRAMES);
    let writer = Writer::create(&path, order.size.0, order.size.1, rate, None).map_err(|problem| problem.to_string())?;
    Ok((grab, Packer::Processor(writer), rate, path))
}

/// The recording itself: the file is made during the count, for the first frame to come as soon
/// as it ends, and then frames go into it at their rate, or as fast as they come, each told to
/// `recorded`. When something goes wrong halfway, what has been recorded is still kept.
fn record(shared: &Shared, order: &Order, mut recorded: impl FnMut(Duration)) -> Result<Option<Made>, String> {
    let (mut grab, mut packer, rate, path) = packing(order)?;
    while shared.clock().now().is_none() {
        if shared.stopped.load(Ordering::Relaxed) {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(5));
    }
    grab.forget_clicks();
    let step = Duration::from_secs(1) / rate;
    let length = SECOND / rate as i64;
    let (mut frames, mut next) = (0u64, Instant::now());
    let mut problem = None;
    while !shared.stopped.load(Ordering::Relaxed) {
        // During a pause the time stands still.
        if shared.clock().now().is_none() {
            thread::sleep(Duration::from_millis(10));
            next = Instant::now();
            continue;
        }
        let now = Instant::now();
        if now < next {
            thread::sleep(next - now);
        }
        let Some(time) = shared.clock().now() else { continue };
        let picture = match grab.take(time) {
            Ok(picture) => picture,
            Err(error) => {
                problem = Some(error.to_string());
                break;
            }
        };
        match packer.write(picture, (time.as_nanos() / 100) as i64, length) {
            Ok(written) => frames += written as u64,
            Err(error) => {
                problem = Some(error.to_string());
                break;
            }
        }
        shared.time.store(time.as_millis() as u64, Ordering::Relaxed);
        recorded(time);
        // A frame that is late doesn't hurry the next ones.
        next = (next + step).max(now);
    }
    let seconds = shared.clock().time().as_secs_f64();
    let way = format!("{} · {}", grab.way(), packer.name());
    let finished = packer.finish().map_err(|error| error.to_string());
    match (frames, finished, problem) {
        (0, _, Some(problem)) => Err(problem),
        // An encoder that hasn't taken a frame has nothing to end its file with.
        (0, _, None) => Ok(None),
        (_, Err(problem), _) => Err(problem),
        (_, Ok(()), _) => Ok(Some(Made { frames, seconds, way, path })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win::video::Reader;

    /// A recording's process at work, here, of a place outside all the monitors: nothing of the
    /// screen is read. It waits for the start, keeps the time, holds it during a pause, and its
    /// file has the frames it says, packed by the graphics card if it has an encoder. Stopped
    /// before the start, it makes nothing.
    #[test]
    fn a_recording_process_follows_the_program() {
        let folder = std::env::temp_dir().join(format!("ainitshot-recorder-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let beyond = crate::win::out_of_sight(windows_sys::Win32::Foundation::RECT { left: 0, top: 100, right: 642, bottom: 462 });
        // Each recording has a file of its own: the card's encoder keeps the file it was given open.
        let order = |name: &str| Order {
            area: [beyond.left + 200, beyond.top, beyond.left + 842, beyond.top + 362],
            size: (642, 362),
            scale: 1.0,
            panel: [0, 0, 10, 10],
            path: folder.join(name),
            card: true,
            frames: 60,
        };
        let run = |order: Order, words: Vec<(u64, &'static str)>| {
            let (reader, mut writer) = std::io::pipe().unwrap();
            writeln!(writer, "{}", serde_json::to_string(&order).unwrap()).unwrap();
            let telling = thread::spawn(move || {
                for (wait, word) in words {
                    thread::sleep(Duration::from_millis(wait));
                    writeln!(writer, "{word}").unwrap();
                }
            });
            let mut said = Vec::new();
            serve_on(BufReader::new(reader), &mut said);
            telling.join().unwrap();
            String::from_utf8(said).unwrap()
        };
        assert_eq!(run(order("stopped.mp4"), vec![(300, "stop")]).trim(), "nothing");

        let said = run(order("recording.mp4"), vec![(200, "go"), (1000, "pause"), (500, "resume"), (1000, "stop")]);
        let last = said.lines().last().unwrap();
        let made: Made = serde_json::from_str(last.strip_prefix("done ").unwrap_or_else(|| panic!("{said}"))).unwrap();
        eprintln!("{made:?}");
        // Two seconds, the pause left out.
        assert!((1.8..2.4).contains(&made.seconds), "{}", made.seconds);
        // GDI reads the screen here, slower than the card does, and the other tests take the
        // processor and the card's encoder too: alone, it has 60 frames a second.
        assert!(made.frames as f64 > made.seconds * 3.0, "{} frames", made.frames);
        let times: Vec<u64> = said.lines().filter_map(|line| line.strip_prefix("time ")?.parse().ok()).collect();
        assert!(times.windows(2).all(|pair| pair[0] <= pair[1]) && *times.last().unwrap() >= 1800, "{times:?}");
        let mut reader = Reader::open(&made.path).unwrap();
        let mut count = 0;
        while reader.next_frame().unwrap().is_some() {
            count += 1;
        }
        assert!(count as u64 + 2 >= made.frames, "{count} of {}", made.frames);
        drop(reader);
        std::fs::remove_dir_all(&folder).ok();
    }
}
