//! The window of a recording that has ended: it goes round in the window, two handles keep a part
//! of it, a switch says GIF or video, and Copy and Save finish it, as they finish a screenshot.
//! Whatever is chosen is made at once on a thread of its own, so that its size can be shown and
//! that Copy and Save have it ready; a change gives up what was being made. The recording itself
//! stays an MP4 file in the temporary folder: a GIF is made from it, and a video only if it is cut.

use std::cell::{Cell, RefCell};
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use serde_json::{Value, json};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
    ICoreWebView2_3, ICoreWebView2Controller, ICoreWebView2Controller2, ICoreWebView2Environment, ICoreWebView2Settings3,
};
use webview2_com::{CreateCoreWebView2ControllerCompletedHandler, WebMessageReceivedEventHandler, take_pwstr};
use windows::Win32::Foundation::{E_POINTER, HWND as ComWindow, RECT as ComRect};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows_core::{HSTRING, Interface, PCWSTR as ComText, PWSTR as ComString};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, HICON, IDC_ARROW,
    LoadCursorW, PostMessageW, RegisterClassExW, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_DPICHANGED, WM_MOVE, WM_SIZE, WNDCLASSEXW, WS_CAPTION,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::record::Recorded;
use super::video::{FRAMES, Reader, SECOND, Writer};
use super::{QUIET, app, browser, clipboard, last_error, out_of_sight, shell, tray, wide};
use crate::gif::{self, Gif};
use crate::text::{self, tr};

const CLASS: PCWSTR = w!("ainitShot.recorded");
const PAGE: &str = include_str!("recorded.html");
/// The address under which the page sees the recording's folder.
const HOST: &str = "recording.ainitshot.example";
/// A file has been made, and how far one being made has come.
const WM_PREPARED: u32 = WM_APP + 1;
const WM_PROGRESS: u32 = WM_APP + 2;
const BACKGROUND: u32 = 0x001C_1C1C;
/// At the scale of 100%: the least width of the window's inside, and the height of what stands
/// under the picture.
const LEAST_WIDTH: i32 = 620;
const CONTROLS: i32 = 96;
/// How much of the monitor's work area the window takes at most.
const MOST_OF_MONITOR: f32 = 0.75;
/// A recording up to this long comes as a GIF at first, a longer one as a video.
const GIF_SECONDS: f64 = 15.0;
/// The frames a second and the width of a GIF: more only makes it much larger.
const GIF_FRAMES: i64 = 15;
const GIF_WIDTH: usize = 1000;
/// The size of a GIF's dimensions in its file.
const GIF_MOST: usize = u16::MAX as usize;
/// What the time kept may be off the whole recording by, and still be the whole recording, in
/// seconds.
const WHOLE: f64 = 0.04;

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Gif,
    Video,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Format::Gif => "gif",
            Format::Video => "mp4",
        }
    }
}

/// What the user wants done with what is made.
#[derive(Clone, Copy)]
enum Deed {
    Copy,
    Save,
}

/// A file made.
struct Made {
    path: PathBuf,
    format: Format,
    bytes: u64,
    size: (usize, usize),
}

struct Window {
    window: HWND,
    controller: Option<ICoreWebView2Controller>,
    icons: Vec<HICON>,
    recorded: Recorded,
    /// The number of what is being made: what has a smaller one gives up.
    number: Arc<AtomicU64>,
    made: Option<Made>,
    failed: Option<String>,
    /// What waits for the file to be made.
    waiting: Option<Deed>,
}

impl Drop for Window {
    fn drop(&mut self) {
        for icon in self.icons.drain(..) {
            unsafe { DestroyIcon(icon) };
        }
    }
}

/// Opens the window of a recording. Only one is open: what was left in an earlier one goes.
pub fn open(recorded: Recorded) {
    close();
    let window = match create_window(&recorded) {
        Ok(window) => window,
        Err(error) => return app::notify(not_open(), &error.to_string()),
    };
    let icons = tray::give_icon(window);
    let number = Arc::new(AtomicU64::new(0));
    WINDOW.with(|cell| {
        *cell.borrow_mut() = Some(Window { window, controller: None, icons, recorded, number, made: None, failed: None, waiting: None });
    });
    unsafe {
        if QUIET {
            ShowWindow(window, SW_SHOWNOACTIVATE);
        } else {
            ShowWindow(window, SW_SHOW);
            SetForegroundWindow(window);
        }
    }
    browser::with_environment(Box::new(move |environment| match environment {
        Some(environment) => create_controller(environment, window),
        None => {
            close();
            app::notify(not_open(), tr("WebView2 didn't start."));
        }
    }));
}

fn not_open() -> &'static str {
    tr("The recording didn't open")
}

fn title() -> String {
    format!("{} — ainitShot", tr("Recording"))
}

/// Closes the window: what has been made stays until the temporary folder is cleaned.
pub fn close() {
    let Some(mut window) = WINDOW.with(|cell| cell.borrow_mut().take()) else { return };
    // What is being made gives up.
    window.number.fetch_add(1, Ordering::Relaxed);
    if let Some(controller) = window.controller.take() {
        let _ = unsafe { controller.Close() };
    }
    unsafe { DestroyWindow(window.window) };
}

fn handle() -> Option<HWND> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref().map(|window| window.window))
}

fn controller() -> Option<ICoreWebView2Controller> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref()?.controller.clone())
}

fn create_controller(environment: &ICoreWebView2Environment, window: HWND) {
    if handle() != Some(window) {
        return;
    }
    let created = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |result, controller| {
        match result.and_then(|()| controller.ok_or_else(|| E_POINTER.into())) {
            Ok(controller) => {
                if let Err(error) = attach(&controller, window) {
                    failed(&error);
                }
            }
            Err(error) => failed(&error),
        }
        Ok(())
    }));
    if let Err(error) = unsafe { environment.CreateCoreWebView2Controller(ComWindow(window), &created) } {
        failed(&error);
    }
}

fn failed(error: &windows_core::Error) {
    close();
    app::notify(not_open(), &error.message());
}

/// Puts the page into the window, as the other windows do, and lets it see the recording's folder.
fn attach(controller: &ICoreWebView2Controller, window: HWND) -> windows_core::Result<()> {
    if handle() != Some(window) {
        return unsafe { controller.Close() };
    }
    unsafe {
        controller.SetBounds(client_rect(window))?;
        let dark = COREWEBVIEW2_COLOR { A: 255, R: 0x1C, G: 0x1C, B: 0x1C };
        controller.cast::<ICoreWebView2Controller2>()?.SetDefaultBackgroundColor(dark)?;
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetIsZoomControlEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.cast::<ICoreWebView2Settings3>()?.SetAreBrowserAcceleratorKeysEnabled(false)?;
        let folder = WINDOW.with(|cell| {
            cell.borrow().as_ref().and_then(|window| window.recorded.path.parent().map(Path::to_path_buf))
        });
        let folder = folder.ok_or_else(|| windows_core::Error::from(E_POINTER))?;
        webview.cast::<ICoreWebView2_3>()?.SetVirtualHostNameToFolderMapping(
            &HSTRING::from(HOST),
            &HSTRING::from(folder.as_path()),
            COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_ALLOW,
        )?;
        let mut token = 0;
        let told = WebMessageReceivedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let mut message = ComString::null();
                if args.TryGetWebMessageAsString(&mut message).is_ok() {
                    heard(&take_pwstr(message));
                }
            }
            Ok(())
        }));
        webview.add_WebMessageReceived(&told, &mut token)?;
        let page = wide(PAGE);
        webview.NavigateToString(ComText(page.as_ptr()))?;
        controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)?;
    }
    WINDOW.with(|cell| {
        if let Some(window) = cell.borrow_mut().as_mut() {
            window.controller = Some(controller.clone());
        }
    });
    Ok(())
}

/// What the page says.
fn heard(message: &str) {
    let Ok(message) = serde_json::from_str::<Value>(message) else { return };
    match message["type"].as_str().unwrap_or_default() {
        "ready" => post(&first_look()),
        "prepare" => {
            let format = if message["format"] == "gif" { Format::Gif } else { Format::Video };
            let (from, to) = (message["from"].as_f64().unwrap_or(0.0), message["to"].as_f64().unwrap_or(0.0));
            prepare(format, from, to);
        }
        "copy" => wish(Deed::Copy),
        "save" => wish(Deed::Save),
        _ => {}
    }
}

/// What the page has to know: the recording, the format to begin with, and how to speak.
fn first_look() -> Value {
    let language = text::current();
    WINDOW.with(|cell| {
        let cell = cell.borrow();
        let Some(window) = cell.as_ref() else { return Value::Null };
        let recorded = &window.recorded;
        let name = recorded.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let rate = recorded.frames as f64 / recorded.seconds.max(0.001);
        json!({
            "type": "init",
            "url": format!("https://{HOST}/{name}"),
            "seconds": recorded.seconds,
            "format": if recorded.seconds <= GIF_SECONDS { "gif" } else { "video" },
            "details": format!("{} fps · {}", rate.round(), recorded.way),
            "language": language.code(),
            "rightToLeft": language.right_to_left(),
            "texts": text::page("recorded"),
        })
    })
}

/// Tells the page something; nothing happens if the page isn't there.
fn post(message: &Value) {
    let Some(controller) = controller() else { return };
    let message = wide(&message.to_string());
    if let Ok(webview) = unsafe { controller.CoreWebView2() } {
        let _ = unsafe { webview.PostWebMessageAsJson(ComText(message.as_ptr())) };
    }
}

/// Starts making what the page has asked for, instead of what was being made.
fn prepare(format: Format, from: f64, to: f64) {
    let started = WINDOW.with(|cell| {
        let mut cell = cell.borrow_mut();
        let window = cell.as_mut()?;
        let number = window.number.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(made) = window.made.take() {
            let _ = fs::remove_file(made.path);
        }
        window.failed = None;
        let source = window.recorded.path.clone();
        let target = source.with_file_name(format!("{number}.{}", format.extension()));
        Some((window.window as isize, window.number.clone(), number, source, target, window.recorded.seconds))
    });
    let Some((target_window, counter, number, source, target, seconds)) = started else { return };
    let whole = from <= WHOLE && to >= seconds - WHOLE;
    thread::spawn(move || {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }.ok();
        let job = Job { window: target_window as HWND, counter, number };
        let range = (seconds_to_time(from), seconds_to_time(to));
        let outcome = match format {
            Format::Gif => job.make_gif(&source, &target, range),
            Format::Video if whole => job.copy(&source, &target),
            Format::Video => job.cut(&source, &target, range),
        };
        let outcome = outcome.map(|size| {
            size.map(|size| Made { bytes: fs::metadata(&target).map_or(0, |file| file.len()), path: target.clone(), format, size })
        });
        // What has given up leaves nothing behind.
        if !matches!(outcome, Ok(Some(_))) {
            let _ = fs::remove_file(&target);
        }
        let outcome = Box::into_raw(Box::new(outcome));
        if unsafe { PostMessageW(job.window, WM_PREPARED, number as usize, outcome as isize) } == 0 {
            drop(unsafe { Box::from_raw(outcome) });
        }
    });
}

fn seconds_to_time(seconds: f64) -> i64 {
    (seconds * SECOND as f64) as i64
}

/// Making a file on a thread of its own.
struct Job {
    window: HWND,
    counter: Arc<AtomicU64>,
    number: u64,
}

/// What a job comes back with: the size of its picture, or nothing if it has given up.
type Outcome = Result<Option<(usize, usize)>, String>;

impl Job {
    fn given_up(&self) -> bool {
        self.counter.load(Ordering::Relaxed) != self.number
    }

    fn progress(&self, done: f64) {
        unsafe { PostMessageW(self.window, WM_PROGRESS, self.number as usize, (done.clamp(0.0, 1.0) * 100.0) as isize) };
    }

    fn copy(&self, source: &Path, target: &Path) -> Outcome {
        fs::copy(source, target).map_err(|error| error.to_string())?;
        let reader = Reader::open(source).map_err(|error| error.to_string())?;
        Ok(Some((reader.width, reader.height)))
    }

    /// A shorter video: what is kept goes through the encoder again.
    fn cut(&self, source: &Path, target: &Path, (from, to): (i64, i64)) -> Outcome {
        let mut reader = Reader::open(source).map_err(|error| error.to_string())?;
        let (width, height) = (reader.width, reader.height);
        let mut writer = Writer::create(target, width as u32, height as u32).map_err(|error| error.to_string())?;
        let length = SECOND / FRAMES as i64;
        while let Some((time, pixels)) = reader.next_frame().map_err(|error| error.to_string())? {
            if self.given_up() {
                return Ok(None);
            }
            if time < from {
                continue;
            }
            if time >= to {
                break;
            }
            writer.write(&pixels, width * 4, time - from, length).map_err(|error| error.to_string())?;
            self.progress((time - from) as f64 / (to - from).max(1) as f64);
        }
        writer.finish().map_err(|error| error.to_string())?;
        Ok(Some((width, height)))
    }

    /// A GIF of what is kept: fewer frames, smaller if need be. A frame gets its delay when the
    /// next one comes.
    fn make_gif(&self, source: &Path, target: &Path, (from, to): (i64, i64)) -> Outcome {
        let mut reader = Reader::open(source).map_err(|error| error.to_string())?;
        let (width, height) = (reader.width, reader.height);
        let (new_width, new_height) = gif_size(width, height);
        let file = File::create(target).map_err(|error| error.to_string())?;
        let mut gif = Gif::new(BufWriter::new(file), new_width as u16, new_height as u16).map_err(|error| error.to_string())?;
        let write = |gif: &mut Gif<BufWriter<File>>, frame: &[u8], delay: i64| {
            gif.frame(frame, delay.max(0) as u32).map_err(|error: io::Error| error.to_string())
        };
        // Hundredths of a second, as the file counts them, from the start of the recording, so
        // that what is rounded off doesn't add up.
        let hundredths = |time: i64| time / (SECOND / 100);
        let step = SECOND / GIF_FRAMES;
        let (mut next, mut waiting) = (from, None::<(Vec<u8>, i64)>);
        while let Some((time, pixels)) = reader.next_frame().map_err(|error| error.to_string())? {
            if self.given_up() {
                return Ok(None);
            }
            if time < next {
                continue;
            }
            if time >= to {
                break;
            }
            let small = match (new_width, new_height) == (width, height) {
                true => pixels,
                false => gif::smaller(&pixels, width, height, new_width, new_height),
            };
            if let Some((frame, at)) = waiting.take() {
                write(&mut gif, &frame, hundredths(time) - hundredths(at))?;
            }
            waiting = Some((small, time));
            while next <= time {
                next += step;
            }
            self.progress((time - from) as f64 / (to - from).max(1) as f64);
        }
        if let Some((frame, at)) = waiting {
            write(&mut gif, &frame, hundredths(to) - hundredths(at))?;
        }
        gif.finish().map_err(|error| error.to_string())?;
        Ok(Some((new_width, new_height)))
    }
}

/// The size of the GIF of a picture.
fn gif_size(width: usize, height: usize) -> (usize, usize) {
    let new_width = width.min(GIF_WIDTH).min(GIF_MOST);
    (new_width, (height * new_width / width.max(1)).clamp(1, GIF_MOST))
}

/// What is being made has come this far.
fn progress(number: u64, percent: usize) {
    if current(number) {
        post(&json!({ "type": "progress", "percent": percent }));
    }
}

fn current(number: u64) -> bool {
    WINDOW.with(|cell| cell.borrow().as_ref().is_some_and(|window| window.number.load(Ordering::Relaxed) == number))
}

/// Something has been made, or making it has failed.
fn prepared(number: u64, outcome: Result<Option<Made>, String>) {
    if !current(number) {
        if let Ok(Some(made)) = outcome {
            let _ = fs::remove_file(made.path);
        }
        return;
    }
    let waiting = WINDOW.with(|cell| {
        let mut cell = cell.borrow_mut();
        let window = cell.as_mut()?;
        match outcome {
            Ok(Some(made)) => {
                post_later(json!({ "type": "prepared", "bytes": made.bytes, "size": format!("{}×{}", made.size.0, made.size.1) }));
                window.made = Some(made);
                window.waiting.take()
            }
            Ok(None) => None,
            Err(problem) => {
                post_later(json!({ "type": "failed", "problem": problem }));
                window.failed = Some(problem);
                window.waiting = None;
                None
            }
        }
    });
    flush();
    if let Some(deed) = waiting {
        finish(deed);
    }
}

thread_local! {
    /// What is to be told to the page once the window isn't being worked on.
    static LATER: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

fn post_later(message: Value) {
    LATER.with(|later| later.borrow_mut().push(message));
}

fn flush() {
    for message in LATER.with(|later| later.take()) {
        post(&message);
    }
}

/// Copy or Save has been pressed: at once if the file is there, or as soon as it is.
fn wish(deed: Deed) {
    let ready = WINDOW.with(|cell| {
        let mut cell = cell.borrow_mut();
        let window = cell.as_mut()?;
        if window.failed.is_some() {
            return None;
        }
        if window.made.is_some() {
            return Some(deed);
        }
        window.waiting = Some(deed);
        None
    });
    if let Some(deed) = ready {
        finish(deed);
    }
}

/// Does what the user has asked with the file made. As with a screenshot, the window goes then.
fn finish(deed: Deed) {
    let Some(window) = handle() else { return };
    let Some(made) = WINDOW.with(|cell| cell.borrow_mut().as_mut()?.made.take()) else { return };
    let save = app::settings().save.clone();
    let name = shell::file_name(&save.name);
    let extension = made.format.extension();
    match deed {
        Deed::Copy => {
            // The file keeps a proper name wherever it is pasted.
            let path = made.path.with_file_name(format!("{name}.{extension}"));
            if let Err(error) = fs::rename(&made.path, &path) {
                app::notify(tr("Couldn't copy"), &error.to_string());
                return give_back(made);
            }
            match clipboard::put_file(window, &path) {
                Ok(()) => close(),
                Err(error) => {
                    app::notify(tr("Couldn't copy"), &error.to_string());
                    give_back(Made { path, ..made });
                }
            }
        }
        Deed::Save => {
            let folder = match save.folder.trim() {
                "" => shell::videos_folder(),
                folder => PathBuf::from(folder),
            };
            let path = if save.ask {
                let Some(path) = shell::ask_where_to_save(window, &folder, &name, extension) else {
                    return give_back(made);
                };
                path
            } else {
                if let Err(error) = fs::create_dir_all(&folder) {
                    app::notify(tr("Couldn't save"), &format!("{}: {error}", folder.display()));
                    return give_back(made);
                }
                shell::new_path(&folder, &name, extension)
            };
            if let Err(error) = fs::copy(&made.path, &path) {
                app::notify(tr("Couldn't save"), &format!("{}: {error}", path.display()));
                return give_back(made);
            }
            let copied = save.copy.then(|| clipboard::put_file(window, &path));
            match copied {
                Some(Err(error)) => app::notify(tr("Couldn't copy"), &error.to_string()),
                copied if !save.ask => app::recording_saved(&path, copied.is_some()),
                _ => {}
            }
            close();
        }
    }
}

/// What was made stays for another try.
fn give_back(made: Made) {
    WINDOW.with(|cell| {
        if let Some(window) = cell.borrow_mut().as_mut() {
            window.made = Some(made);
        }
    });
}

fn create_window(recorded: &Recorded) -> io::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(null());
        if !REGISTERED.get() {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: CreateSolidBrush(BACKGROUND),
                lpszClassName: CLASS,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return Err(last_error());
            }
            REGISTERED.set(true);
        }
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let (place, quiet) = match QUIET {
            true => (out_of_sight(place(recorded, style)), WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE),
            false => (place(recorded, style), 0),
        };
        let window = CreateWindowExW(
            quiet,
            CLASS,
            wide(&title()).as_ptr(),
            style,
            place.left,
            place.top,
            place.right - place.left,
            place.bottom - place.top,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            return Err(last_error());
        }
        let dark: BOOL = 1;
        DwmSetWindowAttribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, (&dark as *const BOOL).cast(), size_of::<BOOL>() as u32);
        Ok(window)
    }
}

/// Where the window appears: in the middle of the monitor under the mouse, as large as the picture
/// allows there.
fn place(recorded: &Recorded, style: u32) -> RECT {
    let mut cursor = POINT::default();
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let (mut dpi, mut unused) = (96, 96);
    unsafe {
        GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
    }
    let work = info.rcWork;
    let scaled = |length: i32| length * dpi as i32 / 96;
    let (room_width, room_height) = ((work.right - work.left) as f32 * MOST_OF_MONITOR, (work.bottom - work.top) as f32 * MOST_OF_MONITOR);
    let controls = scaled(CONTROLS);
    let (picture_width, picture_height) = (recorded.width as f32, recorded.height as f32);
    let fit = (room_width / picture_width).min((room_height - controls as f32).max(1.0) / picture_height).min(1.0);
    let width = ((picture_width * fit) as i32).max(scaled(LEAST_WIDTH));
    let height = (picture_height * fit) as i32 + controls;
    let mut outer = RECT { left: 0, top: 0, right: width, bottom: height };
    unsafe { AdjustWindowRectEx(&mut outer, style, 0, 0) };
    let (width, height) = (outer.right - outer.left, outer.bottom - outer.top);
    let (left, top) = (work.left + (work.right - work.left - width) / 2, work.top + (work.bottom - work.top - height) / 2);
    RECT { left, top, right: left + width, bottom: top + height }
}

fn client_rect(window: HWND) -> ComRect {
    let mut rect = RECT::default();
    unsafe { GetClientRect(window, &mut rect) };
    ComRect { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_SIZE => {
            if let Some(controller) = controller() {
                let _ = unsafe { controller.SetBounds(client_rect(window)) };
            }
        }
        WM_MOVE => {
            if let Some(controller) = controller() {
                let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
            }
        }
        WM_ACTIVATE => {
            let handled = unsafe { DefWindowProcW(window, message, wparam, lparam) };
            if (wparam & 0xFFFF) as u32 != WA_INACTIVE
                && let Some(controller) = controller()
            {
                let _ = unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
            }
            return handled;
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam as *const RECT) };
            let (w, h) = (suggested.right - suggested.left, suggested.bottom - suggested.top);
            unsafe { SetWindowPos(window, null_mut(), suggested.left, suggested.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE) };
        }
        WM_PROGRESS => progress(wparam as u64, lparam as usize),
        WM_PREPARED => {
            let outcome = unsafe { Box::from_raw(lparam as *mut Result<Option<Made>, String>) };
            prepared(wparam as u64, *outcome);
        }
        WM_CLOSE => close(),
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

    use super::super::browser::tests::{evaluate_on, pump_until, save_page_of};
    use super::*;

    fn evaluate(script: &str) -> String {
        evaluate_on(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), script)
    }

    /// Three seconds of a square going across lines of what looks like text.
    fn recording(folder: &Path) -> Recorded {
        let (width, height) = (640usize, 360usize);
        let path = folder.join("recording.mp4");
        let mut writer = Writer::create(&path, width as u32, height as u32).unwrap();
        let length = SECOND / FRAMES as i64;
        for n in 0..90usize {
            let mut pixels = vec![240u8; width * height * 4];
            for y in (40..320).step_by(24) {
                for row in y..y + 8 {
                    for x in 40..600 {
                        if (x / 7) % 5 != 0 {
                            pixels[(row * width + x) * 4..(row * width + x) * 4 + 3].copy_from_slice(&[90, 80, 70]);
                        }
                    }
                }
            }
            let at = 20 + n * 6;
            for y in 120..200 {
                for x in at..at + 80 {
                    pixels[(y * width + x) * 4..(y * width + x) * 4 + 3].copy_from_slice(&[235, 111, 47]);
                }
            }
            writer.write(&pixels, width * 4, n as i64 * length, length).unwrap();
        }
        writer.finish().unwrap();
        Recorded { path, width: width as u32, height: height as u32, seconds: 3.0, frames: 90, way: "made by the test" }
    }

    /// How long a picture of a real screen takes to become a video and a GIF: two seconds of it
    /// going up and three of a small square moving on it, at the size of a whole screen:
    /// `SCREENSHOT=<file.png> cargo test -p ainitshot a_screen_goes -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_screen_goes_into_files_in_time() {
        use std::time::Instant;

        use crate::geometry::Rect;
        use crate::image::Image;
        let file = std::env::var_os("SCREENSHOT").unwrap();
        let picture = Image::from_png(&fs::read(file).unwrap()).unwrap();
        let (width, height) = (picture.width as usize & !1, (picture.height as usize & !1) - 200);
        let folder = std::env::temp_dir().join(format!("recorded-speed-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("recording.mp4");
        let start = Instant::now();
        let mut writer = Writer::create(&path, width as u32, height as u32).unwrap();
        let length = SECOND / FRAMES as i64;
        let count = 5 * FRAMES as usize;
        for n in 0..count {
            let top = (n * 200 / 60).min(200) as i32;
            let shown = picture.crop(Rect::new(0, top, width as i32, top + height as i32));
            let mut pixels = shown.pixels;
            let (x, y) = (100 + n * 7, 100 + n * 3);
            for row in y..y + 16 {
                pixels[(row * width + x) * 4..(row * width + x + 12) * 4].fill(0);
            }
            writer.write(&pixels, width * 4, n as i64 * length, length).unwrap();
        }
        writer.finish().unwrap();
        let encoded = start.elapsed();
        let job = Job { window: null_mut(), counter: Arc::new(AtomicU64::new(1)), number: 1 };
        let start = Instant::now();
        let target = folder.join("out.gif");
        let size = job.make_gif(&path, &target, (0, 5 * SECOND)).unwrap().unwrap();
        let made = start.elapsed();
        let start = Instant::now();
        let cut = folder.join("cut.mp4");
        job.cut(&path, &cut, (SECOND, 4 * SECOND)).unwrap().unwrap();
        let bytes = |path: &Path| fs::metadata(path).unwrap().len() / 1024;
        eprintln!(
            "{width}x{height}: {count} frames written in {encoded:?}, {} KB; a GIF {size:?} in {made:?}, {} KB; a cut of 3 s in {:?}, {} KB",
            bytes(&path),
            bytes(&target),
            start.elapsed(),
            bytes(&cut),
        );
        if let Some(keep) = std::env::var_os("SCREENSHOT_PREVIEW") {
            fs::copy(&target, Path::new(&keep).join("speed.gif")).unwrap();
        }
        fs::remove_dir_all(&folder).unwrap();
    }

    /// Opens the window on a recording made here: the page plays it and seeks in it, a GIF of it
    /// is made, then a GIF of a part, then a video of a part. It starts WebView2, so it runs only
    /// when asked: `LOCALAPPDATA=<folder> cargo test -p ainitshot a_recording_shows -- --ignored`.
    /// With `SCREENSHOT_PREVIEW=<folder>` it keeps what it made there, with a picture of the window.
    #[test]
    #[ignore]
    fn a_recording_shows_and_is_made_into_files() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let kept = std::env::var_os("SCREENSHOT_PREVIEW").map(PathBuf::from);
        let folder = kept.clone().unwrap_or_else(std::env::temp_dir).join(format!("recorded-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        open(recording(&folder));
        let shown = pump_until(30, || controller().is_some() && evaluate("seconds > 0 && video.readyState >= 2") == "true");
        let error = evaluate("String(video.error && video.error.message)");
        let size = evaluate("video.videoWidth + 'x' + video.videoHeight + ' ' + video.duration.toFixed(1)");
        let about = "about.textContent";
        let made_after = |script: &str| {
            evaluate(script);
            pump_until(1, || false);
            pump_until(30, || evaluate(about).contains('·'));
            evaluate(about)
        };
        let gif = made_after("0");
        // The picture is where it was asked to be.
        evaluate("video.pause(); video.currentTime = 2");
        pump_until(5, || evaluate("!video.seeking && Math.abs(video.currentTime - 2) < 0.1") == "true");
        let seeked = evaluate("video.currentTime.toFixed(1)");
        if let Some(folder) = &kept {
            let path = folder.join("recorded.png");
            save_page_of(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), &path.to_string_lossy());
        }
        let part = made_after("to = 1.5; draw(); prepare(); 0");
        let video = made_after("document.querySelector('[data-format=video]').click(); 0");
        let whole = made_after("to = seconds; draw(); prepare(); 0");
        close();
        browser::close();
        pump_until(1, || false);
        if kept.is_none() {
            let _ = fs::remove_dir_all(&folder);
        }
        eprintln!("{size} / {gif} / {part} / {video} / {whole}");
        assert!(shown, "the page shows no recording: {error}");
        assert!(size.starts_with("\"640x360 3."), "{size}");
        assert_eq!(seeked, "\"2.0\"");
        for made in [&gif, &part] {
            assert!(made.contains("640×360"), "{made}");
        }
    }
}
