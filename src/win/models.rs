//! The models the program downloads for what rules can't do: now OpenAI Privacy Filter, with which
//! masking finds names, addresses and other private data, ONNX Runtime, which runs it, and the
//! provider of WebGPU, with which ONNX Runtime runs a model on the graphics card. They live in
//! `%LOCALAPPDATA%\ainitShot\models`. Our own model of the blocks of a window lies next to the
//! program in a test build, in the folder `blocks`.
//!
//! A thread of its own downloads them in the background as the settings want them: a file after a
//! file, each checked against its SHA-256 before it takes its place, so a file in place is a
//! whole one. A download cut short goes on from where it stopped; without a connection it waits a
//! little and tries again. Until a model is downloaded, what needs it does without it. A model is
//! loaded when it is first needed and let go some time after it was last used: loading takes
//! seconds, and a loaded model holds much memory. Each model runs on the graphics card or on the
//! processor, as the settings say; one the card can't run runs on the processor.

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
use crate::blocks::Tree;
use crate::finder::Finder;
use crate::image::Image;
use crate::mask::Found;
use crate::privacy::Filter;
use crate::runtime::Device;
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

/// What the models need, in the order it is downloaded: ONNX Runtime 1.31.0 and the provider of
/// WebGPU 0.6.0 with its compiler of shaders, out of Microsoft's packages on NuGet, then OpenAI
/// Privacy Filter's description and tokens and its weights, quantized to 4 bits, from OpenAI's
/// repository at a fixed commit. ONNX Runtime came from Microsoft's releases on GitHub until they
/// were gone: the file is the same.
const FILTER: [Download; 6] = [
    Download {
        folder: "onnxruntime-1.31.0",
        name: "onnxruntime.dll",
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/1.31.0/microsoft.ml.onnxruntime.1.31.0.nupkg",
        size: 17_253_176,
        sha256: "0c42dc4e278f0968fdf9ebf1b1e54a74c657a9942e70803e453c91993451cf4c",
        zipped: Some((152_948_620, 6_331_918)),
    },
    Download {
        folder: "webgpu-0.6.0",
        name: "onnxruntime_providers_webgpu.dll",
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.ep.webgpu/0.6.0/microsoft.ml.onnxruntime.ep.webgpu.0.6.0.nupkg",
        size: 14_026_040,
        sha256: "6bf102c322d6bad5b1b5a04b0b0c3d43191f70f428ad1d6b4c43719674fc57d9",
        zipped: Some((12_331_830, 5_867_938)),
    },
    Download {
        folder: "webgpu-0.6.0",
        name: "dxcompiler.dll",
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.ep.webgpu/0.6.0/microsoft.ml.onnxruntime.ep.webgpu.0.6.0.nupkg",
        size: 17_986_360,
        sha256: "d833eb71611ae86bcfe73092f8daf5d89c1c084f3b31719b53f9884da64842ba",
        zipped: Some((4_362_520, 7_969_242)),
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

/// The places in the list of ONNX Runtime and of the provider of WebGPU and its compiler.
const RUNTIME: usize = 0;
const PROVIDER: usize = 1;
const COMPILER: usize = 2;
/// The folder of OpenAI Privacy Filter's own files.
const FILTER_FOLDER: &str = "privacy-filter";

/// The version of OpenAI Privacy Filter, as the settings show it: the commit its files come from.
const FILTER_VERSION: &str = "7ffa9a0";

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

/// Whether the settings want each model on the graphics card, and where each ran when it was last
/// loaded: nothing until then, and nothing again once the settings have changed.
struct Cards {
    filter: bool,
    blocks: bool,
    filter_ran: Option<Device>,
    blocks_ran: Option<Device>,
}

static STATUS: Mutex<Status> = Mutex::new(Status { state: State::Off, done: 0, total: 0, speed: 0.0 });
static WISH: Mutex<Wish> = Mutex::new(Wish { wanted: false, paused: false, changed: false });
static CARDS: Mutex<Cards> = Mutex::new(Cards { filter: true, blocks: true, filter_ran: None, blocks_ran: None });
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

/// Runs each model on the graphics card or on the processor, as the settings say: a model loaded
/// on the other one is loaded again the next time it is needed.
pub fn prefer(filter: bool, blocks: bool) {
    let mut cards = lock(&CARDS);
    if cards.filter != filter {
        (cards.filter, cards.filter_ran) = (filter, None);
    }
    if cards.blocks != blocks {
        (cards.blocks, cards.blocks_ran) = (blocks, None);
    }
    drop(cards);
    tell();
}

/// The provider of WebGPU, if a model is wanted on the graphics card and the provider is
/// downloaded.
fn card(wanted: bool) -> Option<PathBuf> {
    (wanted && FILTER[PROVIDER].done() && FILTER[COMPILER].done()).then(|| FILTER[PROVIDER].path())
}

/// Whether a model loaded while the settings wanted the card or not, and running where it does,
/// is to be loaded again: the settings have changed since, or it runs on the processor only
/// because the provider of WebGPU wasn't downloaded yet, and now it is.
fn stale(card_then: bool, device: Device, card_now: bool) -> bool {
    card_then != card_now || (card_now && device == Device::Processor && card(true).is_some())
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
    let ran = |device: Option<Device>| match device {
        Some(Device::Card) => json!("card"),
        Some(Device::Processor) => json!("processor"),
        Some(Device::CardFailed) => json!("card_failed"),
        None => Value::Null,
    };
    let cards = lock(&CARDS);
    json!({
        "state": state,
        "problem": problem,
        "done": status.done,
        "total": status.total,
        "speed": status.speed,
        "version": FILTER_VERSION,
        "blocks": describe_blocks(),
        "ran": { "filter": ran(cards.filter_ran), "blocks": ran(cards.blocks_ran) },
    })
}

/// What the settings window shows of the model of the blocks of a window, whether it can run, which
/// takes the ONNX Runtime that comes with OpenAI Privacy Filter: nothing without the model.
fn describe_blocks() -> Value {
    blocks_folder().map_or(Value::Null, |folder| described(&folder, FILTER[RUNTIME].done()))
}

/// A model in its folder: the version in its `model.json` and the bytes of the folder.
fn described(folder: &Path, runs: bool) -> Value {
    let model: Value = fs::read(folder.join("model.json")).ok().and_then(|text| serde_json::from_slice(&text).ok()).unwrap_or_default();
    let files = fs::read_dir(folder).into_iter().flatten().flatten();
    let size: u64 = files.filter_map(|file| file.metadata().ok()).map(|meta| meta.len()).sum();
    json!({ "version": model["version"], "size": size, "runs": runs })
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
        // Files added to what was downloaded before, as the provider of WebGPU was, come quietly.
        let filter_there = FILTER.iter().filter(|file| file.folder == FILTER_FOLDER).all(Download::done);
        match FILTER.iter().try_for_each(fetch) {
            Ok(()) => {
                count();
                set_state(State::Ready);
                eprintln!("OpenAI Privacy Filter and what runs it are downloaded");
                // The window tells the user, once, when the model itself has come.
                let window = WINDOW.load(Ordering::Acquire);
                if !window.is_null() {
                    unsafe { PostMessageW(window, MESSAGE, usize::from(!filter_there), 0) };
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
    for folder in [FILTER_FOLDER, "onnxruntime-1.31.0", "webgpu-0.6.0"] {
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

struct Loaded<T> {
    model: T,
    used: Instant,
    /// Whether the settings wanted the graphics card when it was loaded, and where it runs.
    card: bool,
    device: Device,
}

static FILTER_LOADED: Mutex<Option<Loaded<Filter>>> = Mutex::new(None);
static BLOCKS_LOADED: Mutex<Option<Loaded<Finder>>> = Mutex::new(None);
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
    let on_card = lock(&CARDS).filter;
    let mut loaded = lock(&FILTER_LOADED);
    if loaded.as_ref().is_some_and(|loaded| stale(loaded.card, loaded.device, on_card)) {
        *loaded = None;
    }
    if loaded.is_none() {
        let started = Instant::now();
        let runtime = FILTER[RUNTIME].path();
        match Filter::load(&runtime, &folder().join(FILTER_FOLDER), card(on_card).as_deref()) {
            Ok(model) => {
                let device = model.device();
                *loaded = Some(Loaded { model, used: started, card: on_card, device });
                ran(|cards| cards.filter_ran = Some(device));
            }
            Err(problem) => return Some(Err(problem)),
        }
        eprintln!("OpenAI Privacy Filter loaded in {:.1} s", started.elapsed().as_secs_f32());
        watch();
    }
    let loaded = loaded.as_mut()?;
    loaded.used = Instant::now();
    Some(loaded.model.find(text))
}

/// The folder of the model of the blocks of a window, if the model is there.
fn blocks_folder() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("blocks");
    beside.join("model.json").is_file().then_some(beside)
}

/// Whether the model of the blocks of a window is there, with what runs it.
pub fn blocks_ready() -> bool {
    FILTER[RUNTIME].done() && blocks_folder().is_some()
}

/// The blocks of a window on its picture, on a monitor of that scale, or nothing without the
/// model. It is loaded the first time and runs for a moment, so it is called away from the
/// window's thread; windows wait for one another.
pub fn blocks(picture: &Image, scale: f32) -> Option<Result<Tree, String>> {
    let folder = blocks_folder()?;
    let on_card = lock(&CARDS).blocks;
    let mut loaded = lock(&BLOCKS_LOADED);
    if loaded.as_ref().is_some_and(|loaded| stale(loaded.card, loaded.device, on_card)) {
        *loaded = None;
    }
    if loaded.is_none() {
        let started = Instant::now();
        match Finder::load(&FILTER[RUNTIME].path(), &folder, card(on_card).as_deref()) {
            Ok(model) => {
                let device = model.device();
                *loaded = Some(Loaded { model, used: started, card: on_card, device });
                ran(|cards| cards.blocks_ran = Some(device));
            }
            Err(problem) => return Some(Err(problem)),
        }
        eprintln!("The model of blocks loaded in {:.1} s", started.elapsed().as_secs_f32());
        watch();
    }
    let loaded = loaded.as_mut()?;
    loaded.used = Instant::now();
    Some(loaded.model.find(picture, scale))
}

/// Notes where a model has come to run, and tells the settings window.
fn ran(how: impl FnOnce(&mut Cards)) {
    how(&mut lock(&CARDS));
    tell();
}

/// Lets the model go once it has lain unused long enough.
fn watch() {
    if WATCHED.swap(true, Ordering::AcqRel) {
        return;
    }
    thread::spawn(|| {
        loop {
            thread::sleep(Duration::from_secs(30));
            let mut filter = lock(&FILTER_LOADED);
            if filter.as_ref().is_some_and(|loaded| loaded.used.elapsed() > KEPT) {
                *filter = None;
                eprintln!("OpenAI Privacy Filter let go");
            }
            drop(filter);
            let mut blocks = lock(&BLOCKS_LOADED);
            if blocks.as_ref().is_some_and(|loaded| loaded.used.elapsed() > KEPT) {
                *blocks = None;
                eprintln!("The model of blocks let go");
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

    /// Downloads ONNX Runtime and the provider of WebGPU with its compiler out of their packages,
    /// and the tokenizer of the model, into a fresh folder of its own, the tokenizer stopped
    /// halfway and gone on with: all come whole. It goes online and downloads 48 MB, so it runs
    /// only when asked: `cargo test -p ainitshot downloads_go_on -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_go_on_where_they_stopped() {
        let root = std::env::temp_dir().join("ainitshot-models-test");
        let _ = fs::remove_dir_all(&root);
        // The test downloads into a folder of its own, never into the program's.
        unsafe { std::env::set_var("LOCALAPPDATA", &root) };
        assert!(folder().starts_with(&root));
        lock(&STATUS).total = FILTER.iter().map(Download::transfer).sum();
        for file in [&FILTER[RUNTIME], &FILTER[PROVIDER], &FILTER[COMPILER]] {
            assert!(fetch(file).is_ok(), "{}", file.name);
            assert!(file.done(), "{}", file.name);
        }
        let tokenizer = FILTER.iter().find(|file| file.name == "tokenizer.json").unwrap();
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
    fn a_model_is_described_from_its_folder() {
        let folder = std::env::temp_dir().join("ainitshot-model-description");
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let description = r#"{ "contract": 4, "version": "0.2", "name": "ainitShot Blocks" }"#;
        fs::write(folder.join("model.json"), description).unwrap();
        fs::write(folder.join("areas-frames.onnx"), [0; 1000]).unwrap();
        let size = 1000 + description.len();
        assert_eq!(described(&folder, true), json!({ "version": "0.2", "size": size, "runs": true }));
        fs::write(folder.join("model.json"), "not a description").unwrap();
        assert_eq!(described(&folder, false)["version"], Value::Null);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_version_shown_is_the_commit_of_the_files() {
        let own: Vec<_> = FILTER.iter().filter(|file| file.folder == FILTER_FOLDER).collect();
        assert_eq!(own.len(), 3);
        assert!(own.iter().all(|file| file.url.contains(FILTER_VERSION)));
    }

    /// A model runs where the settings want it; it is loaded again when they change, and on the
    /// card once the provider of WebGPU has come, but not again after the card couldn't run it.
    #[test]
    fn a_model_is_loaded_again_only_when_its_place_changes() {
        assert!(stale(true, Device::Card, false));
        assert!(stale(false, Device::Processor, true));
        assert!(!stale(true, Device::Card, true));
        assert!(!stale(false, Device::Processor, false));
        assert!(!stale(true, Device::CardFailed, true));
    }

    #[test]
    fn the_whole_download_is_under_a_gigabyte() {
        let total: u64 = FILTER.iter().map(Download::transfer).sum();
        assert!(total > 900_000_000 && total < 1_000_000_000, "{total}");
    }
}
