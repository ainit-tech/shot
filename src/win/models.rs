//! The models the program downloads for what rules can't do: now OpenAI Privacy Filter, with which
//! masking finds names, addresses and other private data, and ONNX Runtime, which runs it. They
//! live in `%LOCALAPPDATA%\ainitShot\models`.
//!
//! A thread of its own downloads them in the background as the settings want them: a file after a
//! file, each checked against its SHA-256 before it takes its place, so a file in place is a
//! whole one. A download cut short goes on from where it stopped; without a connection it waits a
//! little and tries again. Until a model is downloaded, what needs it does without it. A model is
//! loaded when it is first needed and let go some time after it was last used: loading takes
//! seconds, and a loaded model holds much memory.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use super::http::{self, Failure, Get};
use super::{app, overlay, preferences};
use crate::mask::Found;
use crate::privacy::Filter;
use crate::text::tr;
use crate::update::Hash;

/// The message the downloads send to the program's window as they go.
pub const MESSAGE: u32 = WM_APP + 11;
/// How long a model stays loaded after it was last used.
const KEPT: Duration = Duration::from_secs(10 * 60);
/// How long the downloads wait after the program starts, so as not to slow it, and after a
/// failure before they try again.
const FIRST: Duration = Duration::from_secs(20);
const AGAIN: Duration = Duration::from_secs(2 * 60);
/// How often the window hears how a download goes.
const REPORT: Duration = Duration::from_millis(250);

/// A file to download: where it goes, where it comes from, its size and SHA-256. A file that
/// comes in a zip archive is fetched alone: `zipped` gives where its local header is in the
/// archive and its size there, deflated.
struct Download {
    folder: &'static str,
    name: &'static str,
    url: &'static str,
    size: u64,
    sha256: &'static str,
    zipped: Option<(u64, u64)>,
}

impl Download {
    fn path(&self) -> PathBuf {
        folder().join(self.folder).join(self.name)
    }

    /// The bytes that come over the network for the file.
    fn transfer(&self) -> u64 {
        self.zipped.map_or(self.size, |(_, packed)| packed)
    }

    fn done(&self) -> bool {
        fs::metadata(self.path()).is_ok_and(|meta| meta.len() == self.size)
    }
}

/// What OpenAI Privacy Filter needs, in the order it is downloaded: ONNX Runtime 1.31.0 from
/// Microsoft's release, the model's description and tokens, then its weights, quantized to 4 bits,
/// from OpenAI's repository at a fixed commit.
const FILTER: [Download; 4] = [
    Download {
        folder: "onnxruntime-1.31.0",
        name: "onnxruntime.dll",
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.0/onnxruntime-win-x64-1.31.0.zip",
        size: 17_253_176,
        sha256: "0c42dc4e278f0968fdf9ebf1b1e54a74c657a9942e70803e453c91993451cf4c",
        zipped: Some((221_047, 6_178_013)),
    },
    Download {
        folder: "privacy-filter",
        name: "model_q4.onnx",
        url: "https://huggingface.co/openai/privacy-filter/resolve/7ffa9a043d54d1be65afb281eddf0ffbe629385b/onnx/model_q4.onnx",
        size: 160_219,
        sha256: "8f7dee8b46d096f052b359375dfba5d983cc4d18c44a783bf548615c472f8dea",
        zipped: None,
    },
    Download {
        folder: "privacy-filter",
        name: "tokenizer.json",
        url: "https://huggingface.co/openai/privacy-filter/resolve/7ffa9a043d54d1be65afb281eddf0ffbe629385b/tokenizer.json",
        size: 27_868_174,
        sha256: "0614fe83cadab421296e664e1f48f4261fa8fef6e03e63bb75c20f38e37d07d3",
        zipped: None,
    },
    Download {
        folder: "privacy-filter",
        name: "model_q4.onnx_data",
        url: "https://huggingface.co/openai/privacy-filter/resolve/7ffa9a043d54d1be65afb281eddf0ffbe629385b/onnx/model_q4.onnx_data",
        size: 917_120_144,
        sha256: "f30998e28c71c5374cc7e8b7de8f0f83e981592c0c2d652d2ad4928454dbb496",
        zipped: None,
    },
];

/// Where the model stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// The settings don't want it: it isn't downloaded, and what was is removed.
    Off,
    /// To be downloaded, soon or again after a problem.
    Waiting(Option<Problem>),
    Downloading,
    /// Stopped by the user, to go on when the user says.
    Paused,
    Ready,
}

/// What kept a download from finishing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Offline,
    Server,
    /// A file came otherwise than it should: it is downloaded again.
    Damaged,
    /// The folder couldn't be written, or the disk is full.
    Disk,
}

/// The model's state and how far its download has gone, in bytes over the network.
#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub state: State,
    pub done: u64,
    pub total: u64,
    /// Bytes a second, lately.
    pub speed: f64,
}

/// What the settings and the user want of the downloads.
struct Wish {
    wanted: bool,
    paused: bool,
    /// Something changed: the thread has to look again.
    changed: bool,
}

static STATUS: Mutex<Status> = Mutex::new(Status { state: State::Off, done: 0, total: 0, speed: 0.0 });
static WISH: Mutex<Wish> = Mutex::new(Wish { wanted: false, paused: false, changed: false });
static WAKE: Condvar = Condvar::new();
/// Stops the download under way: the wish has changed.
static INTERRUPT: AtomicBool = AtomicBool::new(false);
static WINDOW: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The folder of the models: `%LOCALAPPDATA%\ainitShot\models`.
pub fn folder() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    local.join("ainitShot").join("models")
}

/// Starts the downloads for the program's window, as the settings want OpenAI Privacy Filter.
pub fn start(window: HWND, wanted: bool) {
    WINDOW.store(window, Ordering::Release);
    lock(&WISH).wanted = wanted;
    let total = FILTER.iter().map(Download::transfer).sum();
    lock(&STATUS).total = total;
    thread::spawn(run);
}

/// Wants OpenAI Privacy Filter or not, as the settings say: it is downloaded, or stopped and removed.
pub fn want(wanted: bool) {
    change(|wish| wish.wanted = wanted);
}

/// Pauses the download, or goes on with it.
pub fn pause(paused: bool) {
    change(|wish| wish.paused = paused);
}

fn change(how: impl FnOnce(&mut Wish)) {
    let mut wish = lock(&WISH);
    how(&mut wish);
    wish.changed = true;
    INTERRUPT.store(true, Ordering::Release);
    WAKE.notify_all();
}

pub fn status() -> Status {
    *lock(&STATUS)
}

/// How far OpenAI Privacy Filter has come while it is wanted and not downloaded, from 0 to 1.
pub fn coming() -> Option<f32> {
    let status = status();
    let coming = !matches!(status.state, State::Off | State::Ready);
    coming.then(|| if status.total == 0 { 0.0 } else { status.done as f32 / status.total as f32 })
}

/// The program's window hears how the downloads go: the settings window and the frozen screen
/// show it, and once the model is downloaded, the user is told, once.
pub fn heard(downloaded: bool) {
    preferences::models(&describe());
    overlay::models_changed();
    if downloaded {
        let text = tr("OpenAI Privacy Filter is downloaded: masking finds names and addresses too.");
        app::announce(tr("Smart masking is ready"), text);
    }
}

/// The status for the settings window.
pub fn describe() -> Value {
    let status = status();
    let (state, problem) = match status.state {
        State::Off => ("off", None),
        State::Waiting(problem) => ("waiting", problem),
        State::Downloading => ("downloading", None),
        State::Paused => ("paused", None),
        State::Ready => ("ready", None),
    };
    let problem = problem.map(|problem| match problem {
        Problem::Offline => "offline",
        Problem::Server => "server",
        Problem::Damaged => "damaged",
        Problem::Disk => "disk",
    });
    json!({ "state": state, "problem": problem, "done": status.done, "total": status.total, "speed": status.speed })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn set_state(state: State) {
    let changed = {
        let mut status = lock(&STATUS);
        let changed = status.state != state;
        status.state = state;
        if state != State::Downloading {
            status.speed = 0.0;
        }
        changed
    };
    if changed {
        tell();
    }
}

/// Tells the program's window that the status has changed.
fn tell() {
    let window = WINDOW.load(Ordering::Acquire);
    if !window.is_null() {
        unsafe { PostMessageW(window, MESSAGE, 0, 0) };
    }
}

/// The thread of the downloads: it does what the wish asks, until the program ends.
fn run() {
    let mut first = true;
    loop {
        let (wanted, paused) = {
            let mut wish = lock(&WISH);
            wish.changed = false;
            INTERRUPT.store(false, Ordering::Release);
            (wish.wanted, wish.paused)
        };
        count();
        if !wanted {
            remove();
            set_state(State::Off);
            wait(None);
            continue;
        }
        if FILTER.iter().all(Download::done) {
            set_state(State::Ready);
            wait(None);
            continue;
        }
        if paused {
            set_state(State::Paused);
            wait(None);
            continue;
        }
        if first {
            first = false;
            set_state(State::Waiting(None));
            if wait(Some(FIRST)) {
                continue;
            }
        }
        set_state(State::Downloading);
        match FILTER.iter().try_for_each(fetch) {
            Ok(()) => {
                count();
                set_state(State::Ready);
                eprintln!("OpenAI Privacy Filter is downloaded");
                // The window tells the user, once.
                let window = WINDOW.load(Ordering::Acquire);
                if !window.is_null() {
                    unsafe { PostMessageW(window, MESSAGE, 1, 0) };
                }
            }
            Err(Stop::Asked) => {}
            Err(Stop::Problem(problem)) => {
                eprintln!("The download of OpenAI Privacy Filter stopped: {problem:?}");
                set_state(State::Waiting(Some(problem)));
                wait(Some(AGAIN));
            }
        }
    }
}

/// Waits until the wish changes or, if given, the time passes; whether the wish changed.
fn wait(time: Option<Duration>) -> bool {
    let wish = lock(&WISH);
    let wish = match time {
        Some(time) => WAKE.wait_timeout_while(wish, time, |wish| !wish.changed).unwrap_or_else(PoisonError::into_inner).0,
        None => WAKE.wait_while(wish, |wish| !wish.changed).unwrap_or_else(PoisonError::into_inner),
    };
    wish.changed
}

/// Counts how much of the downloads is done: the files in place, and the parts of a file begun.
fn count() {
    let done = FILTER
        .iter()
        .map(|file| match file.done() {
            true => file.transfer(),
            false if file.zipped.is_none() => fs::metadata(part(&file.path())).map_or(0, |meta| meta.len().min(file.size)),
            false => 0,
        })
        .sum();
    lock(&STATUS).done = done;
    tell();
}

/// Removes the downloaded files, the model let go first.
fn remove() {
    *lock(&FILTER_LOADED) = None;
    for file in &FILTER {
        let _ = fs::remove_file(file.path());
        let _ = fs::remove_file(part(&file.path()));
    }
    for folder in ["privacy-filter", "onnxruntime-1.31.0"] {
        let _ = fs::remove_dir(self::folder().join(folder));
    }
}

/// The file a download goes into until it is checked.
fn part(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

/// Why a download stopped: the user changed the wish, or a problem.
enum Stop {
    Asked,
    Problem(Problem),
}

fn stop(failure: Failure<Stop>) -> Stop {
    match failure {
        Failure::Offline => Stop::Problem(Problem::Offline),
        Failure::Server => Stop::Problem(Problem::Server),
        Failure::Stopped(stop) => stop,
    }
}

/// How fast the bytes come, and tells the window now and then how far the download has gone.
struct Meter {
    last: Instant,
    bytes: u64,
}

impl Meter {
    fn new() -> Meter {
        Meter { last: Instant::now(), bytes: 0 }
    }

    /// Bytes that came: counted, and checked against the wish.
    fn came(&mut self, bytes: usize) -> Result<(), Stop> {
        if INTERRUPT.load(Ordering::Acquire) {
            return Err(Stop::Asked);
        }
        self.bytes += bytes as u64;
        let elapsed = self.last.elapsed();
        let mut status = lock(&STATUS);
        status.done += bytes as u64;
        if elapsed >= REPORT {
            let now = self.bytes as f64 / elapsed.as_secs_f64();
            status.speed = if status.speed == 0.0 { now } else { 0.7 * status.speed + 0.3 * now };
            (self.last, self.bytes) = (Instant::now(), 0);
            drop(status);
            tell();
        }
        Ok(())
    }
}

/// Downloads a file, unless it is in place, and checks it; it takes its place only when whole.
fn fetch(file: &Download) -> Result<(), Stop> {
    if file.done() {
        return Ok(());
    }
    let path = file.path();
    let disk = |_| Stop::Problem(Problem::Disk);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(disk)?;
    }
    let part = part(&path);
    let mut meter = Meter::new();
    let mut hash = Hash::default();
    match file.zipped {
        Some((offset, packed)) => {
            // The local header, the name and the extra field before the data: 30 bytes and two
            // fields of at most a few hundred.
            let get = Get { url: file.url, from: offset, to: Some(offset + 30 + 1024 + packed - 1), limit: packed + 2048, deadline: None };
            let mut zipped = Vec::new();
            http::get_into(&get, &mut |bytes| {
                zipped.extend_from_slice(bytes);
                meter.came(bytes.len())
            })
            .map_err(stop)?;
            let plain = unzip(&zipped, packed).ok_or(Stop::Problem(Problem::Damaged))?;
            hash.update(&plain);
            fs::write(&part, &plain).map_err(disk)?;
        }
        None => {
            // What came before goes into the hash first, and the rest is asked for after it.
            let mut have = fs::metadata(&part).map_or(0, |meta| meta.len());
            if have > file.size {
                fs::remove_file(&part).map_err(disk)?;
                have = 0;
            }
            if have > 0 {
                let mut before = fs::File::open(&part).map_err(disk)?;
                let mut buffer = vec![0u8; 1024 * 1024];
                loop {
                    let read = before.read(&mut buffer).map_err(disk)?;
                    if read == 0 {
                        break;
                    }
                    hash.update(&buffer[..read]);
                }
            }
            let mut out = fs::OpenOptions::new().create(true).append(true).open(&part).map_err(disk)?;
            let get = Get { url: file.url, from: have, to: None, limit: file.size - have, deadline: None };
            http::get_into(&get, &mut |bytes| {
                hash.update(bytes);
                out.write_all(bytes).map_err(disk)?;
                meter.came(bytes.len())
            })
            .map_err(stop)?;
        }
    }
    let whole = fs::metadata(&part).is_ok_and(|meta| meta.len() == file.size) && hash.hex() == file.sha256;
    if !whole {
        let _ = fs::remove_file(&part);
        return Err(Stop::Problem(Problem::Damaged));
    }
    fs::rename(&part, &path).map_err(disk)
}

/// The data of a file in a zip archive from its local header on: stored, or deflated.
fn unzip(zipped: &[u8], packed: u64) -> Option<Vec<u8>> {
    let field = |at: usize| Some(u16::from_le_bytes(zipped.get(at..at + 2)?.try_into().ok()?) as usize);
    if zipped.get(..4)? != b"PK\x03\x04" {
        return None;
    }
    let start = 30 + field(26)? + field(28)?;
    let data = zipped.get(start..start + packed as usize)?;
    match field(8)? {
        0 => Some(data.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec(data).ok(),
        _ => None,
    }
}

struct Loaded {
    filter: Filter,
    used: Instant,
}

static FILTER_LOADED: Mutex<Option<Loaded>> = Mutex::new(None);
static WATCHED: AtomicBool = AtomicBool::new(false);

/// Whether OpenAI Privacy Filter is downloaded, with what runs it.
pub fn filter_ready() -> bool {
    status().state == State::Ready
}

/// For tests, without the thread of the downloads: the model counts as downloaded if its files
/// are in place, as they are once it has been; whether they are.
#[cfg(test)]
pub fn use_downloaded() -> bool {
    let ready = FILTER.iter().all(Download::done);
    if ready {
        lock(&STATUS).state = State::Ready;
    }
    ready
}

/// What OpenAI Privacy Filter marks in the text, or nothing while it isn't downloaded. It is
/// loaded the first time, which takes a few seconds; it runs for a moment, so it is called away
/// from the window's thread.
pub fn find(text: &str) -> Option<Result<Vec<Found>, String>> {
    if !filter_ready() {
        return None;
    }
    let mut loaded = lock(&FILTER_LOADED);
    if loaded.is_none() {
        let started = Instant::now();
        let runtime = FILTER[0].path();
        match Filter::load(&runtime, &folder().join("privacy-filter")) {
            Ok(filter) => *loaded = Some(Loaded { filter, used: started }),
            Err(problem) => return Some(Err(problem)),
        }
        eprintln!("OpenAI Privacy Filter loaded in {:.1} s", started.elapsed().as_secs_f32());
        watch();
    }
    let loaded = loaded.as_mut()?;
    loaded.used = Instant::now();
    Some(loaded.filter.find(text))
}

/// Lets the model go once it has lain unused long enough.
fn watch() {
    if WATCHED.swap(true, Ordering::AcqRel) {
        return;
    }
    thread::spawn(|| {
        loop {
            thread::sleep(Duration::from_secs(30));
            let mut loaded = lock(&FILTER_LOADED);
            if loaded.as_ref().is_some_and(|loaded| loaded.used.elapsed() > KEPT) {
                *loaded = None;
                eprintln!("OpenAI Privacy Filter let go");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in a zip archive comes out of its local header, deflated or stored.
    #[test]
    fn a_file_comes_out_of_its_zip_header() {
        let text = b"OpenAI Privacy Filter, OpenAI Privacy Filter, OpenAI Privacy Filter";
        let packed = miniz_oxide::deflate::compress_to_vec(text, 6);
        let header = |method: u16, data: &[u8]| {
            let mut zipped = b"PK\x03\x04".to_vec();
            zipped.extend_from_slice(&[0; 4]);
            zipped.extend_from_slice(&method.to_le_bytes());
            zipped.extend_from_slice(&[0; 16]);
            zipped.extend_from_slice(&5u16.to_le_bytes());
            zipped.extend_from_slice(&2u16.to_le_bytes());
            zipped.extend_from_slice(b"a.dllxx");
            zipped.extend_from_slice(data);
            zipped.extend_from_slice(b"the next file");
            zipped
        };
        assert_eq!(unzip(&header(8, &packed), packed.len() as u64).as_deref(), Some(&text[..]));
        assert_eq!(unzip(&header(0, text), text.len() as u64).as_deref(), Some(&text[..]));
        assert_eq!(unzip(&header(9, text), text.len() as u64), None);
        assert_eq!(unzip(b"PK\x01\x02", 4), None);
    }

    #[test]
    fn a_part_is_beside_its_file() {
        assert_eq!(part(Path::new(r"C:\m\model_q4.onnx_data")), PathBuf::from(r"C:\m\model_q4.onnx_data.part"));
    }

    /// Downloads ONNX Runtime out of its zip archive and the tokenizer of the model into a fresh
    /// folder of its own, the tokenizer stopped halfway and gone on with: both come whole. It goes
    /// online and downloads 34 MB, so it runs only when asked:
    /// `cargo test -p ainitshot downloads_go_on -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_go_on_where_they_stopped() {
        let root = std::env::temp_dir().join("ainitshot-models-test");
        let _ = fs::remove_dir_all(&root);
        // The test downloads into a folder of its own, never into the program's.
        unsafe { std::env::set_var("LOCALAPPDATA", &root) };
        assert!(folder().starts_with(&root));
        lock(&STATUS).total = FILTER.iter().map(Download::transfer).sum();
        assert!(fetch(&FILTER[0]).is_ok());
        assert!(FILTER[0].done());
        let tokenizer = &FILTER[2];
        let started = lock(&STATUS).done;
        let stopper = thread::spawn(move || {
            while lock(&STATUS).done < started + 10_000_000 {
                thread::sleep(Duration::from_millis(5));
            }
            INTERRUPT.store(true, Ordering::Release);
        });
        assert!(matches!(fetch(tokenizer), Err(Stop::Asked)));
        stopper.join().unwrap();
        let begun = fs::metadata(part(&tokenizer.path())).unwrap().len();
        assert!(begun >= 10_000_000 && begun < tokenizer.size, "{begun}");
        INTERRUPT.store(false, Ordering::Release);
        assert!(fetch(tokenizer).is_ok());
        assert!(tokenizer.done());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_whole_download_is_under_a_gigabyte() {
        let total: u64 = FILTER.iter().map(Download::transfer).sum();
        assert!(total > 900_000_000 && total < 1_000_000_000, "{total}");
    }
}
