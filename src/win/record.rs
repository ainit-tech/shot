//! Recording what is selected (decision 17). The button on the panel of actions closes the frozen
//! screen, and the screen goes on living inside a thin frame. After a count of three the
//! recording runs on a thread of its own, into an MP4 file, with a small panel in place of the
//! panel of actions: the time, the microphone, pause and stop. The hotkey stops it too. What is
//! recorded then goes to the window of the result, where it is cut and becomes a GIF or a video.
//! The frame and the panel stay out of the picture: Windows leaves them out of what is read of the
//! screen.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CombineRgn, CreateRectRgn, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect, PAINTSTRUCT,
    RGN_DIFF, SRCCOPY, SetWindowRgn,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, IDC_ARROW, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, PostMessageW,
    RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetLayeredWindowAttributes, SetTimer,
    SetWindowDisplayAffinity, SetWindowPos, ShowWindow, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE, WM_APP, WM_ERASEBKGND, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_sys::core::PCWSTR;
use windows_sys::w;

use super::gdi::{self, Buffer, Font};
use super::grab::Grab;
use super::video::{self, FRAMES, SECOND, Writer};
use super::voice::{self, Line};
use super::{QUIET, app, last_error, out_of_sight, recorded, shell};
use crate::color::Rgba;
use crate::draw::{self, ToolbarLook};
use crate::geometry::{Point, Rect};
use crate::settings::Icon;
use crate::text::tr;
use crate::toolbar::{Button, Toolbar};

/// What the thread of a recording tells the program's hidden window when it ends.
pub const MESSAGE: u32 = WM_APP + 13;

const FRAME_CLASS: PCWSTR = w!("ainitShot.recording.frame");
const PANEL_CLASS: PCWSTR = w!("ainitShot.recording.panel");
const HINT_CLASS: PCWSTR = w!("ainitShot.recording.hint");
/// The seconds of the count before a recording, for the mouse to get where it should.
const COUNT: u64 = 3;
/// How often the panel is looked at, in milliseconds: often enough to follow a voice.
const TICK: u32 = 50;
const TICK_TIMER: usize = 1;
/// The size a video can have: more than H.264 does everywhere is made smaller. At the other end,
/// an area too small for a video grows.
const MOST: (f32, f32) = (4096.0, 2304.0);
const LEAST: i32 = 32;
/// The sizes on the panel at the scale of 100%: the width of the frame, the size of the time and
/// where the dot and the time stand; the time is followed by a gap before the buttons.
const FRAME_WIDTH: f32 = 2.0;
const TIME_SIZE: f32 = 14.0;
const DOT_CENTRE: f32 = 16.0;
const DOT_RADIUS: f32 = 5.0;
const TIME_LEFT: f32 = 29.0;
const TIME_GAP: f32 = 6.0;
const HINT_SIZE: f32 = 12.0;
const HINT_PADDING: (f32, f32) = (9.0, 5.0);
const HINT_GAP: f32 = 6.0;
/// The colours of the frame and of the dot: red while recording, grey otherwise, as the
/// microphone's.
const RECORDING: Rgba = draw::MIC_LISTENING;
const WAITING: Rgba = draw::MIC_WAITING;
/// The panel's colour, under what is drawn on it, and that of a hint.
const PANEL: Rgba = [32, 32, 32, 255];

thread_local! {
    static ACTIVE: RefCell<Option<Recording>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
    /// Whether the next recording begins with the microphone on, as the last one ended. It is off
    /// when the program starts: nothing listens until the user says so.
    static MICROPHONE: Cell<bool> = const { Cell::new(false) };
}

/// The time of a recording, the same for the picture and the sound. It stands still before the
/// recording begins and during a pause.
#[derive(Default)]
struct Clock {
    begun: Option<Instant>,
    paused: Option<Instant>,
    /// The pauses that are over.
    left_out: Duration,
}

impl Clock {
    fn time(&self) -> Duration {
        let Some(begun) = self.begun else { return Duration::ZERO };
        self.paused.unwrap_or_else(Instant::now).saturating_duration_since(begun).saturating_sub(self.left_out)
    }

    /// The time, while it goes.
    fn now(&self) -> Option<Duration> {
        (self.begun.is_some() && self.paused.is_none()).then(|| self.time())
    }
}

/// What the program and the threads of a recording say to each other.
#[derive(Default)]
struct Shared {
    clock: Mutex<Clock>,
    /// The recording is to end.
    stopped: AtomicBool,
    /// How much has been recorded, in milliseconds.
    time: AtomicU64,
    /// The microphone.
    line: Line,
}

impl Shared {
    fn clock(&self) -> MutexGuard<'_, Clock> {
        self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A recording, finished.
pub struct Recorded {
    /// The MP4 file, in a temporary folder of its own that lasts as long as the window of the
    /// result.
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// Its length in seconds, and how many frames it has in them.
    pub seconds: f64,
    pub frames: u64,
    /// The way the screen was seen.
    pub way: &'static str,
    /// The microphone's file, which is in the video too, if it was on at any moment.
    pub voice: Option<PathBuf>,
}

/// What the thread of a recording comes back with: nothing, if it was stopped before it began, and
/// why its sound has been lost, if it has.
struct Ending {
    shared: Arc<Shared>,
    outcome: Result<Option<Recorded>, String>,
    trouble: Option<String>,
}

/// The windows of a recording going on.
struct Recording {
    frame: HWND,
    panel: HWND,
    hint: HWND,
    shared: Arc<Shared>,
    /// When the count began.
    begins: Instant,
    /// The panel's own place on the desktop, the monitor it is on and its scale.
    place: Rect,
    monitor: Rect,
    scale: f32,
    /// Its picture, and the picture of the hint with its text.
    buffer: Buffer,
    font: Font,
    hint_font: Font,
    hint_text: Vec<u16>,
    room: i32,
    hovered: Option<Button>,
    /// What the panel last showed, not to draw it again for nothing.
    shown: Option<Shown>,
}

/// What the panel shows: the time, whether it is paused, the button under the mouse and the
/// microphone's level, in steps.
type Shown = (String, bool, Option<Button>, Option<u32>);

/// The size of the panel of a recording.
pub fn panel_size(scale: f32) -> (i32, i32) {
    let bar = Toolbar::recording(room(scale), false, scale);
    (bar.rect.width(), bar.rect.height())
}

/// The room before the buttons of the panel, for the dot and the time.
fn room(scale: f32) -> i32 {
    let width = Buffer::new(1, 1).ok().zip(Font::new(px(TIME_SIZE, scale), true).ok()).map_or(0, |(buffer, font)| {
        let longest: Vec<u16> = "00:00".encode_utf16().collect();
        gdi::text_size(buffer.dc(), &font, &longest).0
    });
    px(TIME_LEFT, scale) + width + px(TIME_GAP, scale)
}

fn px(size: f32, scale: f32) -> i32 {
    (size * scale).round() as i32
}

/// Whether a recording is going on, its count included.
pub fn active() -> bool {
    ACTIVE.with(|cell| cell.try_borrow().map_or(true, |active| active.is_some()))
}

/// Starts a recording of `area` of the desktop, with its panel at `panel` on `monitor`.
pub fn start(area: Rect, panel: Rect, monitor: Rect, scale: f32) {
    if active() {
        return;
    }
    let path = match shell::new_recording() {
        Ok(path) => path,
        Err(error) => return app::notify(not_started(), &error.to_string()),
    };
    let (area, size) = fitted(area);
    let shared = Arc::new(Shared::default());
    shared.line.wanted.store(MICROPHONE.get(), Ordering::Relaxed);
    let recording = match Recording::create(area, panel, monitor, scale, shared.clone()) {
        Ok(recording) => recording,
        Err(error) => return app::notify(not_started(), &error.to_string()),
    };
    ACTIVE.with(|cell| *cell.borrow_mut() = Some(recording));
    let target = app::main_window() as isize;
    thread::spawn(move || {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }.ok();
        let voice_file = path.with_file_name(voice::FILE);
        let listening = {
            let (shared, voice_file) = (shared.clone(), voice_file.clone());
            thread::spawn(move || {
                unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }.ok();
                voice::listen(&shared.line, || shared.stopped.load(Ordering::Relaxed), || shared.clock().now(), &voice_file)
            })
        };
        let mut outcome = capture(&shared, area, size, &path, scale, panel);
        // However it has ended, the microphone goes.
        shared.stopped.store(true, Ordering::Relaxed);
        let voiced = listening.join().unwrap_or(false);
        let mut trouble = None;
        if let (true, Ok(Some(recorded))) = (voiced, &mut outcome) {
            trouble = add_voice(recorded, &voice_file).err();
        }
        // Nothing has come of it: its folder goes.
        if !matches!(outcome, Ok(Some(_)))
            && let Some(folder) = path.parent()
        {
            let _ = std::fs::remove_dir_all(folder);
        }
        let ending = Box::into_raw(Box::new(Ending { shared, outcome, trouble }));
        if unsafe { PostMessageW(target as HWND, MESSAGE, 0, ending as isize) } == 0 {
            drop(unsafe { Box::from_raw(ending) });
        }
    });
}

fn not_started() -> &'static str {
    tr("The recording didn't start")
}

fn no_microphone() -> &'static str {
    tr("The microphone isn't available")
}

/// Puts the microphone's sound into a finished recording. If that fails, it stays without sound.
fn add_voice(recorded: &mut Recorded, voice: &Path) -> Result<(), String> {
    let mixed = recorded.path.with_file_name("mixed.mp4");
    video::mix(&recorded.path, voice, &mixed).map_err(|error| error.to_string())?;
    std::fs::rename(&mixed, &recorded.path).map_err(|error| error.to_string())?;
    recorded.voice = Some(voice.to_path_buf());
    Ok(())
}

/// What a video of the area can be: its size even, as H.264 wants, and neither too small nor too
/// large. The area changes with the size, unless the picture has to be made smaller.
fn fitted(area: Rect) -> (Rect, (u32, u32)) {
    let (width, height) = (area.width().max(LEAST), area.height().max(LEAST));
    let shrink = (MOST.0 / width as f32).min(MOST.1 / height as f32);
    if shrink < 1.0 {
        let even = |length: i32| ((length as f32 * shrink) as u32 & !1).max(2);
        return (area, (even(width), even(height)));
    }
    let (width, height) = (width & !1, height & !1);
    (Rect::new(area.left, area.top, area.left + width, area.top + height), (width as u32, height as u32))
}

/// The thread of a recording: the file is made during the count, for the first frame to come as
/// soon as it ends, and then frames go into it at their rate, or as fast as they come. When
/// something goes wrong halfway, what has been recorded is still kept.
fn capture(shared: &Shared, area: Rect, size: (u32, u32), path: &Path, scale: f32, panel: Rect) -> Result<Option<Recorded>, String> {
    let mut grab = Grab::new(area, size, scale, panel).map_err(|error| error.to_string())?;
    let mut writer = Writer::create(path, size.0, size.1, None).map_err(|error| error.to_string())?;
    while shared.clock().now().is_none() {
        if shared.stopped.load(Ordering::Relaxed) {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(10));
    }
    grab.forget_clicks();
    let way = grab.way();
    let step = Duration::from_secs(1) / FRAMES;
    let length = SECOND / FRAMES as i64;
    let (mut frames, mut next) = (0u64, Instant::now());
    let mut problem = None;
    while !shared.stopped.load(Ordering::Relaxed) {
        // During a pause the time stands still.
        if shared.clock().now().is_none() {
            thread::sleep(Duration::from_millis(15));
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
        let stride = picture.width as usize * 4;
        if let Err(error) = writer.write(picture.pixels, stride, (time.as_nanos() / 100) as i64, length) {
            problem = Some(error.to_string());
            break;
        }
        frames += 1;
        shared.time.store(time.as_millis() as u64, Ordering::Relaxed);
        // A frame that is late doesn't hurry the next ones.
        next = (next + step).max(now);
    }
    let seconds = shared.clock().time().as_secs_f64();
    let finished = writer.finish().map_err(|error| error.to_string());
    match (frames, finished, problem) {
        (0, _, Some(problem)) | (_, Err(problem), _) => Err(problem),
        (0, Ok(()), None) => Ok(None),
        (_, Ok(()), _) => Ok(Some(Recorded { path: path.to_path_buf(), width: size.0, height: size.1, seconds, frames, way, voice: None })),
    }
}

/// The hotkey, or a click on the icon: a recording going on stops, and nothing else happens.
pub fn stop() -> bool {
    let Some(recording) = ACTIVE.with(|cell| cell.try_borrow_mut().ok()?.take()) else { return false };
    recording.shared.stopped.store(true, Ordering::Relaxed);
    recording.close();
    true
}

/// A recording has ended.
pub fn heard(ending: LPARAM) {
    let Ending { shared, outcome, trouble } = *unsafe { Box::from_raw(ending as *mut Ending) };
    // If it ended by itself, its windows go now.
    let own = ACTIVE.with(|cell| cell.try_borrow().is_ok_and(|active| active.as_ref().is_some_and(|r| Arc::ptr_eq(&r.shared, &shared))));
    if own {
        stop();
    }
    if let Some(problem) = trouble {
        app::notify(no_microphone(), &problem);
    }
    match outcome {
        Ok(Some(recorded)) => recorded::open(recorded),
        Ok(None) => {}
        Err(problem) => app::notify(tr("The recording stopped"), &problem),
    }
}

impl Recording {
    fn create(area: Rect, place: Rect, monitor: Rect, scale: f32, shared: Arc<Shared>) -> std::io::Result<Recording> {
        register();
        let thin = px(FRAME_WIDTH, scale).max(1);
        let outer = Rect::new(area.left - thin, area.top - thin, area.right + thin, area.bottom + thin);
        // The frame and the hint are never in the way of the mouse.
        let through = WS_EX_LAYERED | WS_EX_TRANSPARENT;
        let frame = window(FRAME_CLASS, outer, through)?;
        let panel = window(PANEL_CLASS, place, 0)?;
        let hint = window(HINT_CLASS, Rect::new(place.left, place.top, place.left + 1, place.top + 1), through)?;
        unsafe {
            // Only the line around the area is the window: everything inside stays under the mouse.
            let (whole, inner) = (CreateRectRgn(0, 0, outer.width(), outer.height()), CreateRectRgn(thin, thin, thin + area.width(), thin + area.height()));
            CombineRgn(whole, whole, inner, RGN_DIFF);
            DeleteObject(inner);
            if SetWindowRgn(frame, whole, 0) == 0 {
                DeleteObject(whole);
            }
            for window in [frame, hint] {
                SetLayeredWindowAttributes(window, 0, 255, LWA_ALPHA);
            }
            for window in [frame, panel, hint] {
                SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE);
            }
            let round = DWMWCP_ROUND;
            for window in [panel, hint] {
                DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&raw const round).cast(), size_of_val(&round) as u32);
            }
            ShowWindow(frame, SW_SHOWNOACTIVATE);
            ShowWindow(panel, SW_SHOWNOACTIVATE);
            SetTimer(panel, TICK_TIMER, TICK, None);
        }
        Ok(Recording {
            frame,
            panel,
            hint,
            shared,
            begins: Instant::now(),
            place,
            monitor,
            scale,
            buffer: Buffer::new(place.width() as u32, place.height() as u32)?,
            font: Font::new(px(TIME_SIZE, scale), true)?,
            hint_font: Font::new(px(HINT_SIZE, scale), false)?,
            hint_text: Vec::new(),
            room: room(scale),
            hovered: None,
            shown: None,
        })
    }

    fn close(self) {
        unsafe {
            DestroyWindow(self.hint);
            DestroyWindow(self.panel);
            DestroyWindow(self.frame);
        }
    }

    /// The count still left, in whole seconds, while it lasts.
    fn counting(&self) -> Option<u64> {
        let gone = self.begins.elapsed().as_secs();
        (self.shared.clock().begun.is_none() && gone < COUNT).then(|| COUNT - gone)
    }

    fn paused(&self) -> bool {
        self.shared.clock().paused.is_some()
    }

    /// How loud the microphone hears, from 0 to 1, while it is on.
    fn voice(&self) -> Option<f32> {
        let line = &self.shared.line;
        line.wanted.load(Ordering::Relaxed).then(|| line.level.load(Ordering::Relaxed) as f32 / 1000.0)
    }

    /// What the panel shows now.
    fn look(&self) -> Shown {
        (self.text(), self.paused(), self.hovered, self.voice().map(|level| (level * 20.0) as u32))
    }

    /// What the panel says: the count, or the time.
    fn text(&self) -> String {
        match self.counting() {
            Some(left) => left.to_string(),
            None => {
                let seconds = self.shared.time.load(Ordering::Relaxed) / 1000;
                format!("{:02}:{:02}", seconds / 60, seconds % 60)
            }
        }
    }

    fn bar(&self) -> Toolbar {
        Toolbar::recording(self.room, self.paused(), self.scale)
    }

    /// A tick: the count ends when its time comes, and the panel shows what has changed.
    fn tick(&mut self) {
        if self.counting().is_none() && self.shared.clock().begun.is_none() {
            self.shared.clock().begun = Some(Instant::now());
            unsafe { InvalidateRect(self.frame, null(), 0) };
        }
        let problem = self.shared.line.problem.lock().ok().and_then(|mut told| told.take());
        if let Some(problem) = problem {
            MICROPHONE.set(false);
            app::notify(no_microphone(), &problem);
        }
        if self.shown.as_ref() != Some(&self.look()) {
            unsafe { InvalidateRect(self.panel, null(), 0) };
        }
    }

    fn colour(&self) -> Rgba {
        if self.shared.clock().now().is_some() { RECORDING } else { WAITING }
    }

    fn paint_panel(&mut self) {
        let look = self.look();
        let bar = self.bar();
        let (colour, counting, voice) = (self.colour(), self.counting().is_some(), self.voice());
        compose(&mut self.buffer, &self.font, &bar, &look.0, colour, counting, self.hovered, voice);
        self.shown = Some(look);
        let mut paint = PAINTSTRUCT::default();
        unsafe {
            let dc = BeginPaint(self.panel, &mut paint);
            BitBlt(dc, 0, 0, bar.rect.width(), bar.rect.height(), self.buffer.dc(), 0, 0, SRCCOPY);
            EndPaint(self.panel, &paint);
        }
    }

    fn paint_frame(&self) {
        let mut paint = PAINTSTRUCT::default();
        let mut client = RECT::default();
        let [red, green, blue, _] = self.colour();
        unsafe {
            let dc = BeginPaint(self.frame, &mut paint);
            GetClientRect(self.frame, &mut client);
            let brush = CreateSolidBrush(red as u32 | (green as u32) << 8 | (blue as u32) << 16);
            FillRect(dc, &client, brush);
            DeleteObject(brush);
            EndPaint(self.frame, &paint);
        }
    }

    fn paint_hint(&self) {
        let mut paint = PAINTSTRUCT::default();
        let mut client = RECT::default();
        let [red, green, blue, _] = PANEL;
        unsafe {
            let dc = BeginPaint(self.hint, &mut paint);
            GetClientRect(self.hint, &mut client);
            let brush = CreateSolidBrush(red as u32 | (green as u32) << 8 | (blue as u32) << 16);
            FillRect(dc, &client, brush);
            DeleteObject(brush);
            let (x, y) = (px(HINT_PADDING.0, self.scale), px(HINT_PADDING.1, self.scale));
            gdi::draw_text(dc, &self.hint_font, &self.hint_text, x, y, draw::TEXT);
            EndPaint(self.hint, &paint);
        }
    }

    /// The mouse has moved over the panel, or left it.
    fn hover(&mut self, button: Option<Button>) {
        if self.hovered == button {
            return;
        }
        self.hovered = button;
        unsafe { InvalidateRect(self.panel, null(), 0) };
        let Some(button) = button else {
            unsafe { ShowWindow(self.hint, SW_HIDE) };
            return;
        };
        if button == Button::Pause && self.counting().is_some() {
            unsafe { ShowWindow(self.hint, SW_HIDE) };
            return;
        }
        self.hint_text = hint_of(button, self.voice().is_some()).encode_utf16().collect();
        let size = gdi::text_size(self.buffer.dc(), &self.hint_font, &self.hint_text);
        let (padding, gap) = ((px(HINT_PADDING.0, self.scale), px(HINT_PADDING.1, self.scale)), px(HINT_GAP, self.scale));
        let (width, height) = (size.0 + 2 * padding.0, size.1 + 2 * padding.1);
        let anchor = self.bar().button(button).unwrap_or_default().offset(self.place.left, self.place.top);
        let left = (anchor.left + anchor.width() / 2 - width / 2).min(self.monitor.right - width).max(self.monitor.left);
        let above = self.place.top - gap - height;
        let top = if above >= self.monitor.top { above } else { self.place.bottom + gap };
        unsafe {
            SetWindowPos(self.hint, null_mut(), left, top, width, height, SWP_NOACTIVATE | SWP_NOZORDER);
            InvalidateRect(self.hint, null(), 0);
            ShowWindow(self.hint, SW_SHOWNOACTIVATE);
        }
    }

    /// A click on the panel.
    fn click(&mut self, button: Option<Button>) -> bool {
        match button {
            Some(Button::Microphone) => {
                let on = self.voice().is_none();
                self.shared.line.wanted.store(on, Ordering::Relaxed);
                MICROPHONE.set(on);
            }
            Some(Button::Pause) if self.counting().is_none() => self.shared.clock().paused = Some(Instant::now()),
            Some(Button::Resume) => {
                let mut clock = self.shared.clock();
                if let Some(since) = clock.paused.take() {
                    clock.left_out += since.elapsed();
                }
            }
            Some(Button::Stop) => return true,
            _ => return false,
        }
        unsafe {
            InvalidateRect(self.frame, null(), 0);
            InvalidateRect(self.panel, null(), 0);
            ShowWindow(self.hint, SW_HIDE);
        }
        // The button under the mouse has changed.
        self.hovered = None;
        false
    }
}

/// Draws the panel of a recording: the dot, what it says, and the buttons.
#[allow(clippy::too_many_arguments)]
fn compose(buffer: &mut Buffer, font: &Font, bar: &Toolbar, text: &str, colour: Rgba, counting: bool, hovered: Option<Button>, voice: Option<f32>) {
    let scale = bar.scale;
    {
        let mut frame = buffer.frame();
        let [red, green, blue, alpha] = PANEL;
        frame.pixels.chunks_exact_mut(4).for_each(|pixel| pixel.copy_from_slice(&[blue, green, red, alpha]));
        let queries: [Icon; 0] = [];
        let look = ToolbarLook {
            tool: None,
            hovered,
            color: PANEL,
            width: 0.0,
            can_undo: false,
            can_pause: !counting,
            voice,
            busy: None,
            listening: false,
            queries: &queries,
            coming: None,
        };
        draw::toolbar(&mut frame, bar, &look);
        let middle = bar.rect.height() as f32 / 2.0;
        draw::mic_dot(&mut frame, (DOT_CENTRE * scale, middle), DOT_RADIUS * scale, colour);
    }
    let words: Vec<u16> = text.encode_utf16().collect();
    let height = gdi::text_size(buffer.dc(), font, &words).1;
    gdi::draw_text(buffer.dc(), font, &words, px(TIME_LEFT, scale), (bar.rect.height() - height) / 2, draw::TEXT);
}

/// What a button of the panel does.
fn hint_of(button: Button, microphone: bool) -> String {
    match button {
        Button::Microphone if microphone => tr("Turn the microphone off").into(),
        Button::Microphone => tr("Turn the microphone on").into(),
        Button::Pause => tr("Pause").into(),
        Button::Resume => tr("Go on recording").into(),
        _ => format!("{} · {}", tr("Stop"), app::settings().hotkey),
    }
}

fn window(class: PCWSTR, place: Rect, extra: WINDOW_EX_STYLE) -> std::io::Result<HWND> {
    let place = RECT { left: place.left, top: place.top, right: place.right, bottom: place.bottom };
    let place = if QUIET { out_of_sight(place) } else { place };
    let style = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | extra;
    let (width, height) = (place.right - place.left, place.bottom - place.top);
    let made = unsafe {
        CreateWindowExW(style, class, null(), WS_POPUP, place.left, place.top, width, height, null_mut(), null_mut(), GetModuleHandleW(null()), null())
    };
    if made.is_null() { Err(last_error()) } else { Ok(made) }
}

fn register() {
    if REGISTERED.replace(true) {
        return;
    }
    for (class, procedure) in [
        (FRAME_CLASS, frame_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
        (PANEL_CLASS, panel_proc),
        (HINT_CLASS, hint_proc),
    ] {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: unsafe { GetModuleHandleW(null()) },
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
            lpszClassName: class,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
    }
}

/// Does something with the recording going on. Windows calls the procedures again from inside
/// some of the calls made here: such nested messages find nothing to do.
fn with<T>(work: impl FnOnce(&mut Recording) -> T) -> Option<T> {
    ACTIVE.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(work))
}

fn point(lparam: LPARAM) -> Point {
    Point::new((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn frame_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT if with(|recording| recording.paint_frame()).is_some() => 0,
        WM_ERASEBKGND => 1,
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe extern "system" fn hint_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT if with(|recording| recording.paint_hint()).is_some() => 0,
        WM_ERASEBKGND => 1,
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe extern "system" fn panel_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT if with(Recording::paint_panel).is_some() => 0,
        WM_ERASEBKGND => 1,
        WM_TIMER => {
            with(Recording::tick);
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_MOUSEMOVE => {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: window,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&mut track) };
            with(|recording| {
                let button = recording.bar().hit(point(lparam));
                recording.hover(button);
            });
            0
        }
        WM_MOUSELEAVE => {
            with(|recording| recording.hover(None));
            0
        }
        WM_LBUTTONUP => {
            let stopping = with(|recording| {
                let button = recording.bar().hit(point(lparam));
                recording.click(button)
            });
            if stopping == Some(true) {
                stop();
            }
            0
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Image;

    /// The marks of a click as they go, on a light, a dark and a blue ground, as the recording
    /// has them: with `SCREENSHOT_PREVIEW=<folder>` the picture stays there.
    #[test]
    fn the_marks_of_clicks_show_on_any_ground() {
        let (cell, phases) = (120usize, [0.0f32, 0.05, 0.1, 0.2, 0.3, 0.45]);
        let grounds: [[u8; 3]; 3] = [[245, 245, 245], [32, 32, 32], [235, 111, 47]];
        let (width, height) = (cell * phases.len(), cell * grounds.len());
        let mut pixels = vec![255u8; width * height * 4];
        for (row, ground) in grounds.iter().enumerate() {
            for line in pixels[row * cell * width * 4..(row + 1) * cell * width * 4].chunks_exact_mut(4) {
                line[..3].copy_from_slice(ground);
            }
        }
        let original = pixels.clone();
        let mut frame = draw::Frame { pixels: &mut pixels, width: width as u32, height: height as u32 };
        for row in 0..grounds.len() {
            for (column, &time) in phases.iter().enumerate() {
                let at = ((column * cell + cell / 2) as f32, (row * cell + cell / 2) as f32);
                let mut clicks = crate::clicks::Clicks::default();
                clicks.update(0.0, at, true, true);
                clicks.update(time, at, false, true);
                for mark in clicks.marks(time, 1.0) {
                    draw::click_mark(&mut frame, &mark, 1.0);
                }
            }
        }
        // In every cell something has been drawn, and only near the click.
        for row in 0..grounds.len() {
            for column in 0..phases.len() {
                let start = ((row * cell) * width + column * cell) * 4;
                assert_eq!(pixels[start..start + 4], original[start..start + 4], "{row} {column}");
                let changed = (0..cell * cell).any(|n| {
                    let at = ((row * cell + n / cell) * width + column * cell + n % cell) * 4;
                    pixels[at..at + 3] != original[at..at + 3]
                });
                assert!(changed, "{row} {column}");
            }
        }
        if let Some(folder) = std::env::var_os("SCREENSHOT_PREVIEW") {
            let picture = Image { width: width as u32, height: height as u32, pixels };
            std::fs::write(std::path::Path::new(&folder).join("clicks.png"), picture.to_png().unwrap()).unwrap();
        }
    }

    /// What is read of the screen has no fourth byte to speak of: GDI leaves it empty, the graphics
    /// card fills it. A mark looks the same on both.
    #[test]
    fn a_mark_looks_the_same_whatever_the_fourth_byte() {
        let drawn = |fourth: u8| {
            let mut pixels: Vec<u8> = [200u8, 180, 160, fourth].repeat(80 * 80);
            let mut frame = draw::Frame { pixels: &mut pixels, width: 80, height: 80 };
            let mut clicks = crate::clicks::Clicks::default();
            clicks.update(0.0, (40.0, 40.0), true, true);
            clicks.update(0.1, (40.0, 40.0), false, true);
            for mark in clicks.marks(0.1, 1.0) {
                draw::click_mark(&mut frame, &mark, 1.0);
            }
            pixels.chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect::<Vec<u8>>()
        };
        assert!(drawn(255) == drawn(0));
    }

    #[test]
    fn the_time_stands_still_before_the_start_and_during_a_pause() {
        let mut clock = Clock::default();
        assert_eq!(clock.now(), None);
        assert_eq!(clock.time(), Duration::ZERO);
        let start = Instant::now() - Duration::from_secs(10);
        clock.begun = Some(start);
        assert!(clock.now().unwrap() >= Duration::from_secs(10));
        // Paused at the seventh second of a recording of ten.
        clock.paused = Some(start + Duration::from_secs(7));
        assert_eq!(clock.now(), None);
        assert_eq!(clock.time(), Duration::from_secs(7));
        // Going on after two seconds of pause, the time is two seconds behind the clock.
        clock.paused = None;
        clock.left_out = Duration::from_secs(2);
        let time = clock.now().unwrap();
        assert!((Duration::from_secs(8)..Duration::from_secs(9)).contains(&time), "{time:?}");
    }

    #[test]
    fn a_video_has_a_size_it_can_have() {
        // An odd size loses a row, and the area with it.
        let (area, size) = fitted(Rect::new(100, 200, 741, 563));
        assert_eq!((area, size), (Rect::new(100, 200, 740, 562), (640, 362)));
        // What is too small grows.
        assert_eq!(fitted(Rect::new(0, 0, 5, 5)).1, (32, 32));
        // What is too large stays whole, and is made smaller.
        let wide = Rect::new(0, 0, 7680, 2160);
        let (area, (width, height)) = fitted(wide);
        assert_eq!(area, wide);
        assert_eq!((width % 2, height % 2), (0, 0));
        assert!(width <= 4096 && height <= 2304 && (width as f32 / height as f32 - 7680.0 / 2160.0).abs() < 0.01);
    }

    /// The panel as it counts, as it records with the mouse on Stop, and as it waits. With
    /// `SCREENSHOT_PREVIEW=<folder>` the pictures stay there.
    #[test]
    fn the_panel_of_a_recording_shows_what_goes_on() {
        for (name, scale, text, colour, paused, counting, hovered, voice) in [
            ("count", 1.0, "3", WAITING, false, true, None, None),
            ("going", 1.0, "01:27", RECORDING, false, false, Some(Button::Stop), None),
            ("voice", 1.0, "01:27", RECORDING, false, false, None, Some(0.7)),
            ("paused", 1.5, "12:04", WAITING, true, false, Some(Button::Resume), Some(0.1)),
        ] {
            let bar = Toolbar::recording(room(scale), paused, scale);
            assert!(bar.rect.width() > bar.rect.height() * 2, "{}", bar.rect.width());
            let (width, height) = (bar.rect.width() as u32, bar.rect.height() as u32);
            let mut buffer = Buffer::new(width, height).unwrap();
            let font = Font::new(px(TIME_SIZE, scale), true).unwrap();
            compose(&mut buffer, &font, &bar, text, colour, counting, hovered, voice);
            // The buttons stand after the room for the time, in the panel.
            assert!(bar.buttons.iter().all(|(_, rect)| rect.left >= room(scale) && rect.right <= bar.rect.right));
            if let Some(folder) = std::env::var_os("SCREENSHOT_PREVIEW") {
                let picture = Image { width, height, pixels: buffer.frame().pixels.to_vec() };
                std::fs::write(std::path::Path::new(&folder).join(format!("panel-{name}.png")), picture.to_png().unwrap()).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod flow {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::IsRectEmpty;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};

    use super::super::browser;
    use super::super::browser::tests::pump_until;
    use super::*;

    unsafe extern "system" fn receiver(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == MESSAGE {
            heard(lparam);
            return 0;
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn text() -> Option<String> {
        with(|recording| recording.text())
    }

    /// The whole way of a recording, its windows out of sight, of a place outside all the monitors:
    /// nothing of the screen is read. The count ends, the time goes, pause holds it, Stop ends it
    /// and the window of the result opens; a recording stopped during the count leaves nothing.
    /// It starts WebView2, so it runs only when asked:
    /// `LOCALAPPDATA=<folder> cargo test -p ainitshot a_recording_goes -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_recording_goes_from_the_count_to_its_window() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(receiver),
            hInstance: unsafe { GetModuleHandleW(null()) },
            lpszClassName: w!("ainitShot.test.receiver"),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
        let main = unsafe {
            CreateWindowExW(0, class.lpszClassName, null(), 0, 0, 0, 0, 0, null_mut(), null_mut(), class.hInstance, null())
        };
        assert!(!main.is_null());
        app::set_main_window(main);
        let beyond = out_of_sight(RECT { left: 0, top: 100, right: 640, bottom: 460 });
        let area = Rect::new(beyond.left + 200, beyond.top, beyond.right + 200, beyond.bottom);
        let monitor = Rect::new(0, 0, 1920, 1080);
        let panel = Rect::new(100, 100, 100 + panel_size(1.0).0, 100 + panel_size(1.0).1);

        // Stopped during the count, it leaves nothing.
        start(area, panel, monitor, 1.0);
        assert!(active());
        assert_eq!(text().as_deref(), Some("3"));
        pump_until(1, || false);
        assert!(stop());
        assert!(!active() && !stop());
        pump_until(1, || false);
        assert!(!recorded::is_open());

        start(area, panel, monitor, 1.0);
        let shown = with(|recording| unsafe {
            let mut place = RECT::default();
            GetWindowRect(recording.panel, &mut place);
            (IsWindowVisible(recording.frame) != 0, IsWindowVisible(recording.panel) != 0, IsRectEmpty(&place) == 0)
        });
        assert_eq!(shown, Some((true, true, true)));
        let begun = pump_until(5, || with(|recording| recording.shared.clock().begun.is_some()) == Some(true));
        assert!(begun, "the count didn't end");
        // The time goes.
        assert!(pump_until(3, || text().as_deref() == Some("00:01")), "{:?}", text());
        // The mouse on Stop, as Windows tells it, and a hint for it.
        with(|recording| {
            let (bar, room) = (recording.bar(), recording.room);
            let stop = bar.button(Button::Stop).unwrap();
            assert!(stop.left >= room);
            recording.hover(Some(Button::Stop));
            assert!(unsafe { IsWindowVisible(recording.hint) } != 0);
            assert!(!recording.hint_text.is_empty());
            recording.hover(None);
        });
        // A pause holds the time, and the button turns into going on.
        with(|recording| recording.click(Some(Button::Pause)));
        assert_eq!(with(|recording| recording.bar().button(Button::Resume).is_some()), Some(true));
        pump_until(1, || false);
        let held = with(|recording| recording.shared.time.load(Ordering::Relaxed));
        pump_until(1, || false);
        assert_eq!(with(|recording| recording.shared.time.load(Ordering::Relaxed)), held);
        with(|recording| recording.click(Some(Button::Resume)));
        pump_until(1, || false);
        assert!(with(|recording| recording.shared.time.load(Ordering::Relaxed)) > held);
        let stopping = with(|recording| recording.click(Some(Button::Stop)));
        assert_eq!(stopping, Some(true));
        assert!(stop());
        let opened = pump_until(15, recorded::is_open);
        assert!(opened, "the window of the result didn't open");
        let frames = recorded::frames();
        pump_until(5, || false);
        recorded::close();
        browser::close();
        pump_until(1, || false);
        eprintln!("{frames:?}");
        let (frames, seconds) = frames.unwrap();
        // A second before the pause and a second after it.
        assert!((1.5..3.0).contains(&seconds), "{seconds}");
        // As many frames as the time had, give or take.
        assert!(frames as f64 > seconds * 20.0, "{frames} in {seconds}");
    }
}
