//! The window over all monitors that shows the frozen screen. It turns the mouse and the keys
//! into changes of the selection, drawing and the actions of the toolbar; the selection, the
//! annotations and how things look live outside Windows, in `selection`, `annotation` and `draw`.

use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::thread;

use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CLOAK, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, BitBlt, EndPaint, InvalidateRect, PAINTSTRUCT, SRCCOPY, UpdateWindow};
use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows_sys::Win32::UI::Input::Ime::{
    CFS_POINT, COMPOSITIONFORM, IACE_DEFAULT, ImmAssociateContextEx, ImmGetContext, ImmReleaseContext,
    ImmSetCompositionWindow,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, ReleaseCapture, SetCapture, VK_A, VK_BACK, VK_C, VK_CONTROL, VK_DELETE, VK_END,
    VK_ESCAPE, VK_HOME, VK_LBUTTON, VK_LEFT, VK_M, VK_RBUTTON, VK_RETURN, VK_RIGHT, VK_S, VK_SHIFT, VK_V, VK_Y, VK_Z,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, GetCursorPos, GetSystemMetrics, HTCLIENT, HWND_TOPMOST, IDC_APPSTARTING,
    IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, KillTimer,
    LoadCursorW, PostMessageW, RegisterClassExW, SM_SWAPBUTTON, SW_HIDE, SWP_SHOWWINDOW, SetCursor, SetForegroundWindow,
    SetTimer, SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CAPTURECHANGED, WM_CHAR, WM_ERASEBKGND,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT,
    WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::capture::{self, Screen};
use super::gdi::{self, Buffer, Font};
use super::browser::{self, Heard};
use super::{app, card, clipboard, last_error, masking, models, ocr, shell};
use crate::annotation::{self, Annotation, Draft, Grip, History, Label, Shape, Tool};
use crate::color::Rgba;
use crate::draw::{self, Text, ToolbarLook};
use crate::geometry::{Point, PointF, Rect};
use crate::image::Image;
use crate::query::{self, Way};
use crate::selection::{Hit, Selection};
use crate::settings::{Icon, Mode, QueryButton, Settings};
use crate::text::tr;
use crate::toolbar::{Button, Palette, Toolbar};

const CLASS: PCWSTR = w!("ainitShot.overlay");
/// The answer to a query has come: the query's number in `wparam`, the answer in a box in
/// `lparam`.
const WM_ANSWER: u32 = WM_APP + 2;
/// The text of the screenshot has been read: a [`Read`] in a box in `lparam`.
const WM_READ: u32 = WM_APP + 4;
/// The private data of the screenshot has been looked for: a [`Masked`] in a box in `lparam`.
const WM_MASKED: u32 = WM_APP + 5;
const HINT_TIMER: usize = 1;
const RING_TIMER: usize = 2;
const HOLD_TIMER: usize = 3;
const HELD_TIMER: usize = 4;
const NOTE_TIMER: usize = 5;
const CARET_TIMER: usize = 6;
/// How often the caret of the text being written blinks, in milliseconds.
const CARET_BLINK: u32 = 530;
/// How long the mouse rests on a button before its hint shows, in milliseconds.
const HINT_DELAY: u32 = 450;
/// How long a button for a conversation is held before its question is asked aloud, in
/// milliseconds; let go sooner, it asks the question of the settings.
const HOLD_TIME: u32 = 350;
/// How often the held key or mouse button is looked at, in case it was let go over another
/// window, in milliseconds.
const HELD_TICK: u32 = 100;
/// How long a note about a question asked aloud stays, in milliseconds.
const NOTE_TIME: u32 = 3000;
/// The most of the words heard that the frozen screen shows, in letters: the last ones.
const WORDS_SHOWN: usize = 60;
/// How long the ring with the line width stays after the wheel turns, in milliseconds.
const RING_TIME: u32 = 700;
/// A selection smaller than this both ways, in pixels, was a click: it selects the window under it.
const CLICK: i32 = 5;
/// How far from a handle of the annotation picked the mouse still takes it, in pixels at 100%.
const HANDLE_REACH: f32 = 8.0;
/// How far from what an annotation draws the mouse still takes it, in pixels at 100%.
const TOUCH_REACH: f32 = 5.0;
/// How far the mouse moves with an annotation taken before the annotation follows, in pixels at
/// 100%: a click that trembles moves nothing.
const DRAG_START: f32 = 3.0;

/// Where the browser goes, or what went wrong.
type Answer = Result<Way, String>;
/// The text read in the screenshot, or what went wrong.
type Read = Result<String, String>;
/// What covers the private data of the screenshot, on the screen, or what went wrong.
type Masked = Result<Vec<Rect>, String>;

thread_local! {
    static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
}

/// The overlay window. It lives as long as the program and holds a session while it is shown.
struct Overlay {
    window: HWND,
    session: Option<Session>,
    /// The colour and the line width carry over from one screenshot to the next.
    color: usize,
    width: Option<f32>,
    /// Counts queries, so that the answer to one that was cancelled is recognised.
    queries: u64,
}

/// One screenshot, from the hotkey until the overlay closes.
struct Session {
    screen: Screen,
    buffer: Buffer,
    settings: Rc<Settings>,
    colors: Vec<Rgba>,
    color: usize,
    /// The line width at the scale of 100%.
    width: f32,
    selection: Selection,
    tool: Option<Tool>,
    history: History,
    /// The selections started over, the last one last.
    earlier: Vec<Earlier>,
    draft: Option<Draft>,
    layers: Option<Layers>,
    draft_changed: bool,
    hovered: Option<Button>,
    show_hint: bool,
    palette: bool,
    ring: Option<Point>,
    sending: Option<Sending>,
    /// A button for a conversation held down, until it is let go or held long enough to ask aloud.
    pressing: Option<Press>,
    /// A question asked aloud, while its button is held and until it has gone.
    listening: Option<Listening>,
    /// The button whose text of the screenshot is being read: copying it, or masking.
    reading: Option<Button>,
    /// What happened to the last question asked aloud, or to reading the text, next to its
    /// button for a few seconds.
    note: Option<(Button, String)>,
    /// Text being written with the text tool, until it is put down.
    typing: Option<Typing>,
    /// The annotation that shows its handles: the one drawn last, or the one clicked.
    picked: Option<usize>,
    /// An annotation being moved or reshaped with the mouse.
    grabbed: Option<Grab>,
    /// While nothing is selected, the window under the mouse, which a click selects.
    snap: Option<Rect>,
    /// The window under the mouse where a new selection began, selected if the mouse doesn't
    /// move before it is let go.
    snap_press: Option<Rect>,
    /// What everything around the selection covered when it was last drawn.
    decorated: Option<Rect>,
    fonts: Vec<((i32, bool), Font)>,
}

/// The annotations drawn into pictures the size of the selection: the finished ones, and those
/// with the one being drawn on top. While an annotation is dragged, the finished layer holds
/// those under it and `above` those over it, so that only it is drawn again as it moves.
struct Layers {
    origin: Point,
    finished: Pixmap,
    above: Option<Pixmap>,
    current: Pixmap,
}

/// Text being written: where the selection was clicked, what is typed so far and where in it
/// the caret is, its colour and the line width its size follows, how it looks, and the caret.
/// A label being rewritten keeps its place among the annotations and how it was, for Esc.
struct Typing {
    at: Point,
    text: String,
    /// Where typed letters go: a byte of the text where a letter begins, or its end.
    position: usize,
    color: Rgba,
    width: f32,
    label: Option<Annotation>,
    caret: Rect,
    caret_on: bool,
    editing: Option<(usize, Annotation)>,
}

/// An annotation being dragged: which one, by what, where the mouse took it, how it was, and
/// whether the mouse has gone far enough for it to follow.
struct Grab {
    index: usize,
    grip: Grip,
    from: PointF,
    original: Annotation,
    moving: bool,
}

/// A selection started over, with what was drawn on it and the tool chosen: undo brings it back.
struct Earlier {
    selection: Rect,
    history: History,
    tool: Option<Tool>,
}

/// A query on its way: its screenshot is kept for the clipboard in case it doesn't get through.
struct Sending {
    id: u64,
    button: usize,
    picture: Image,
}

/// What holds a button down: a key or the mouse.
#[derive(Clone, Copy, PartialEq)]
enum Holder {
    Key(u16),
    Mouse,
}

/// A button for a conversation held down.
struct Press {
    button: usize,
    holder: Holder,
}

/// A question asked aloud: the microphone is on only while its button is held, and the page
/// finishes listening and sends the question once it is let go.
struct Listening {
    id: u64,
    button: usize,
    holder: Holder,
    picture: Image,
    /// Whether the microphone is on.
    on: bool,
    /// The words heard so far.
    words: String,
    /// Whether the button has been let go.
    released: bool,
}

/// Everything around the selection, laid out for drawing.
struct Scene {
    selection: Option<Rect>,
    scale: f32,
    /// The window a click would select, lit up while nothing is selected.
    snap: Option<(Rect, f32)>,
    /// The annotation picked, and what its handles or its dashed frame cover.
    picked: Option<(usize, Rect)>,
    label: Option<(Rect, Text)>,
    toolbar: Option<Toolbar>,
    /// The panel of the actions: masking, the query buttons, copying and saving.
    actions: Option<Toolbar>,
    palette: Option<Palette>,
    hint: Option<(Rect, Text)>,
    ring: Option<(Point, f32)>,
}

pub fn create(instance: HINSTANCE) -> io::Result<()> {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            // Double clicks rewrite labels.
            style: CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(last_error());
        }
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS,
            w!("ainitShot"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            return Err(last_error());
        }
        // The overlay appears at once, without the animation of ordinary windows.
        set_flag(window, DWMWA_TRANSITIONS_FORCEDISABLED, true);
        OVERLAY.with(|overlay| {
            *overlay.borrow_mut() = Some(Overlay { window, session: None, color: 0, width: None, queries: 0 });
        });
    }
    Ok(())
}

/// Freezes the screen and shows the overlay over it. Shown already, it starts the selection over
/// on the same frozen screen: the hotkey has been pressed again.
pub fn open() {
    let opened = OVERLAY.with(|cell| {
        let mut cell = cell.try_borrow_mut().ok()?;
        let overlay = cell.as_mut()?;
        let window = overlay.window;
        if let Some(session) = &mut overlay.session {
            // While a screenshot is being sent or a question asked, it waits, as clicks do.
            if !session.busy() {
                session.start_over(window);
            }
            return None;
        }
        let session = match Session::start(app::settings(), overlay.color, overlay.width) {
            Ok(session) => session,
            Err(error) => {
                app::notify(tr("The screen didn't freeze"), &error.to_string());
                return None;
            }
        };
        let place = session.screen.image.bounds().offset(session.screen.origin.x, session.screen.origin.y);
        let mut session = session;
        // The window under the mouse is lit up at once, and remembered as drawn, so that it
        // goes dark again when the mouse leaves it.
        session.snap = session.screen.window_at(session.cursor());
        session.refresh(overlay.window);
        overlay.session = Some(session);
        Some((overlay.window, place))
    });
    let Some((window, place)) = opened else { return };
    input_method(window, false);
    unsafe {
        // Shown hidden from view and painted first, so that the screen doesn't flash.
        set_flag(window, DWMWA_CLOAK, true);
        SetWindowPos(window, HWND_TOPMOST, place.left, place.top, place.width(), place.height(), SWP_SHOWWINDOW);
        UpdateWindow(window);
        set_flag(window, DWMWA_CLOAK, false);
        SetForegroundWindow(window);
        SetCursor(LoadCursorW(null_mut(), IDC_CROSS));
    }
    // While the user selects, a page kept ready that has grown old is made fresh.
    browser::prepare();
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == browser::WM_HEARD {
        // Taken over first, like an answer.
        let heard = unsafe { Box::from_raw(lparam as *mut Heard) };
        OVERLAY.with(|cell| {
            if let Ok(mut overlay) = cell.try_borrow_mut()
                && let Some(overlay) = overlay.as_mut()
            {
                overlay.heard(*heard);
            }
        });
        return 0;
    }
    if message == WM_READ {
        // Taken over first, like an answer.
        let read = unsafe { Box::from_raw(lparam as *mut Read) };
        OVERLAY.with(|cell| {
            if let Ok(mut overlay) = cell.try_borrow_mut()
                && let Some(overlay) = overlay.as_mut()
            {
                overlay.text_read(*read);
            }
        });
        return 0;
    }
    if message == WM_MASKED {
        // Taken over first, like an answer.
        let masked = unsafe { Box::from_raw(lparam as *mut Masked) };
        OVERLAY.with(|cell| {
            if let Ok(mut overlay) = cell.try_borrow_mut()
                && let Some(overlay) = overlay.as_mut()
            {
                overlay.masked(*masked);
            }
        });
        return 0;
    }
    if message == WM_ANSWER {
        // The answer is taken over first, so that it is freed even if nobody waits for it.
        let answer = unsafe { Box::from_raw(lparam as *mut Answer) };
        OVERLAY.with(|cell| {
            if let Ok(mut overlay) = cell.try_borrow_mut()
                && let Some(overlay) = overlay.as_mut()
            {
                overlay.answered(wparam as u64, *answer);
            }
        });
        return 0;
    }
    // Windows calls this function again from inside some of the calls made while handling a
    // message; such nested messages get the default handling.
    let handled = OVERLAY.with(|cell| match cell.try_borrow_mut() {
        Ok(mut overlay) => overlay.as_mut().and_then(|overlay| overlay.handle(message, wparam, lparam)),
        Err(_) => None,
    });
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(window, message, wparam, lparam) })
}

/// What a key asks for.
enum Action {
    Click(Button),
    Hold(usize, Holder),
    Hush,
    ClosePalette,
    Close,
    Copy,
    CopyText,
    Save,
    /// Saving where the user says, whatever the settings.
    SaveAs,
    Mask,
    Undo,
    Redo,
    SelectMonitor,
    Remove,
}

impl Overlay {
    fn handle(&mut self, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match message {
            WM_PAINT => self.paint(),
            WM_ERASEBKGND => return Some(1),
            WM_SETCURSOR if (lparam & 0xFFFF) as u32 == HTCLIENT => {
                self.set_cursor();
                return Some(1);
            }
            WM_LBUTTONDOWN => self.press(point(lparam)),
            WM_LBUTTONDBLCLK => self.double_click(point(lparam)),
            WM_MOUSEMOVE => self.mouse_move(point(lparam)),
            WM_LBUTTONUP => self.release(),
            WM_KEYUP => self.let_go(Holder::Key(wparam as u16)),
            WM_CHAR => self.typed(wparam as u32),
            WM_CAPTURECHANGED => self.end_drag(),
            WM_RBUTTONUP => self.right_click(),
            WM_MOUSEWHEEL => self.wheel((wparam >> 16) as u16 as i16),
            // A key held down comes again and again; only its first press counts for holding.
            WM_KEYDOWN => self.key(wparam as u16, lparam & (1 << 30) != 0),
            WM_TIMER => self.timer(wparam),
            // Switching to another window cancels the screenshot.
            WM_ACTIVATE if (wparam & 0xFFFF) as u32 == WA_INACTIVE => self.close(),
            _ => return None,
        }
        Some(0)
    }

    fn paint(&mut self) {
        unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(self.window, &mut paint);
            if let Some(session) = &mut self.session {
                let area = from_win(paint.rcPaint).intersect(session.screen.image.bounds());
                if !area.is_empty() {
                    session.render(area);
                    let (x, y) = (area.left, area.top);
                    BitBlt(dc, x, y, area.width(), area.height(), session.buffer.dc(), x, y, SRCCOPY);
                }
            }
            EndPaint(self.window, &paint);
        }
    }

    fn set_cursor(&self) {
        let shape = self.session.as_ref().map_or(IDC_ARROW, Session::cursor_shape);
        unsafe { SetCursor(LoadCursorW(null_mut(), shape)) };
    }

    fn press(&mut self, p: Point) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.busy() {
            return;
        }
        session.show_hint = false;
        session.snap_press = None;
        // A click puts down the text being written, unless it picks a colour for it.
        if !session.palette && session.button_at(p) != Some(Button::Color) {
            session.put_down_text(window);
        }
        if session.palette {
            // A click anywhere closes the palette; on a colour it also picks that colour.
            if let Some(color) = session.palette_layout().and_then(|palette| palette.hit(p)) {
                session.color = color;
                if let Some(typing) = &mut session.typing {
                    typing.color = session.colors[color];
                }
                session.retype(window);
            }
            session.palette = false;
            session.refresh(window);
            return;
        }
        if let Some(button) = session.button_at(p) {
            match button {
                Button::Query(i) if session.converses(i) => {
                    unsafe { SetCapture(window) };
                    self.hold(i, Holder::Mouse);
                }
                _ => self.click(button),
            }
            return;
        }
        // An annotation under the mouse is taken: by a handle of the one picked, with any tool,
        // or whole, with no tool chosen; a label also with the text tool, which rewrites the
        // label when it is only clicked.
        if let Some((index, grip)) = session.annotation_at(p) {
            session.grab(window, index, grip, p);
            unsafe { SetCapture(window) };
            return;
        }
        session.unpick(window);
        let reach = session.reach(p);
        let selection = session.selection.rect().filter(|r| !r.is_empty());
        match (selection, session.selection.hit(p, reach), session.tool) {
            (Some(_), Hit::Edges(_), _) | (Some(_), Hit::Inside, None) => session.selection.press(p, reach),
            // The text tool writes where the selection is clicked.
            (Some(_), Hit::Inside, Some(Tool::Text)) => return session.start_typing(window, p),
            (Some(selection), Hit::Inside, Some(tool)) => {
                let scale = session.screen.monitor_at(p).scale;
                let color = session.colors[session.color];
                session.draft = Draft::start(tool, inside(p, selection), color, session.width * scale);
                session.draft_changed = true;
            }
            // With annotations on the selection a click outside it does nothing, so that they
            // aren't lost by accident.
            (Some(_), Hit::Outside, _) if !session.history.is_empty() => return,
            _ => {
                session.snap_press = session.screen.window_at(p);
                session.selection.press(p, reach);
            }
        }
        unsafe { SetCapture(window) };
        session.refresh(window);
    }

    fn mouse_move(&mut self, p: Point) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if let Some(grab) = &mut session.grabbed {
            let Some(selection) = session.selection.rect() else { return };
            let at = inside(p, selection);
            if !grab.moving && at.distance(grab.from) < DRAG_START * session.screen.monitor_at(p).scale {
                return;
            }
            grab.moving = true;
            let changed = match grab.grip {
                Grip::Body => grab.original.moved(at.x - grab.from.x, at.y - grab.from.y),
                // The handle keeps its place under the mouse where it was taken; Shift keeps a
                // line at 45° and a rectangle square.
                Grip::Handle(handle) => {
                    let taken = grab.original.handles().get(handle).copied().unwrap_or(grab.from);
                    let to = PointF::new(at.x + taken.x - grab.from.x, at.y + taken.y - grab.from.y);
                    grab.original.reshaped(handle, to, key_down(VK_SHIFT))
                }
            };
            let index = grab.index;
            let before = session.history.items().get(index).map_or(Rect::default(), Annotation::bounds);
            let after = changed.bounds();
            session.history.set(index, changed);
            session.draft_changed = true;
            invalidate(window, before.union(after).intersect(selection));
            session.refresh(window);
        } else if let Some(draft) = &mut session.draft {
            let Some(selection) = session.selection.rect() else { return };
            let before = draft.annotation().bounds();
            draft.extend(inside(p, selection), key_down(VK_SHIFT));
            let after = draft.annotation().bounds();
            session.draft_changed = true;
            invalidate(window, before.union(after).intersect(selection));
        } else if session.selection.is_dragging() {
            session.selection.drag_to(p);
            session.refresh(window);
        } else {
            let snap = if session.selection.rect().is_none() { session.screen.window_at(p) } else { None };
            if snap != session.snap {
                session.snap = snap;
                session.refresh(window);
            }
            let hovered = session.button_at(p);
            if hovered != session.hovered {
                session.hovered = hovered;
                session.show_hint = false;
                unsafe {
                    KillTimer(window, HINT_TIMER);
                    if hovered.is_some() {
                        SetTimer(window, HINT_TIMER, HINT_DELAY, None);
                    }
                }
                session.refresh(window);
            }
        }
    }

    fn release(&mut self) {
        unsafe { ReleaseCapture() };
        self.end_drag();
        self.let_go(Holder::Mouse);
    }

    fn end_drag(&mut self) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if let Some(grab) = session.grabbed.take() {
            // A click that moved nothing only picks the annotation, and leaves nothing to undo;
            // on a label with the text tool it rewrites the label.
            session.history.forget_if_unchanged();
            session.layers = None;
            let label = matches!(grab.original.shape, Shape::Text(_));
            if !grab.moving && grab.grip == Grip::Body && label && session.tool == Some(Tool::Text) {
                return session.edit_label(window, grab.index);
            }
            session.refresh(window);
        } else if let Some(draft) = session.draft.take() {
            let before = draft.annotation().bounds();
            // Shift keeps the magic pencil's loop as drawn.
            match draft.finish(key_down(VK_SHIFT)) {
                Some(annotation) => {
                    invalidate(window, before.union(annotation.bounds()));
                    session.history.push(annotation);
                    // The annotation drawn shows its handles, to be put right at once.
                    session.picked = Some(session.history.items().len() - 1);
                }
                None => {
                    invalidate(window, before);
                    // A click that drew nothing picks the annotation under it, to show its handles.
                    session.picked = session.touched(session.cursor(), |_| true);
                }
            }
            session.layers = None;
            session.refresh(window);
        } else if session.selection.is_dragging() {
            session.selection.release();
            // A click without dragging, or with the hand trembling, selects the window under it.
            let clicked = session.selection.rect().is_none_or(|r| r.width() < CLICK && r.height() < CLICK);
            if let Some(snapped) = session.snap_press.take().filter(|_| clicked) {
                session.selection.select(snapped);
            }
            session.snap = None;
            session.refresh(window);
        }
    }

    fn right_click(&mut self) {
        let window = self.window;
        match &mut self.session {
            Some(session) if session.busy() => {}
            Some(session) if session.palette => {
                session.palette = false;
                session.refresh(window);
            }
            // The first right click starts over, the next one closes the overlay.
            Some(session) if session.selection.rect().is_some() => session.start_over(window),
            _ => self.close(),
        }
    }

    fn wheel(&mut self, delta: i16) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        let step = delta as f32 / 120.0;
        session.width = (session.width + step).clamp(1.0, 50.0);
        // The text being written grows and shrinks with the width, and the next text is written
        // in its size.
        if let Some(typing) = &mut session.typing {
            typing.width = (typing.width + step).clamp(1.0, 50.0);
            session.width = typing.width;
        }
        session.retype(window);
        session.ring = Some(session.cursor());
        unsafe { SetTimer(window, RING_TIMER, RING_TIME, None) };
        session.refresh(window);
    }

    fn timer(&mut self, id: usize) {
        let window = self.window;
        unsafe { KillTimer(window, id) };
        match id {
            HOLD_TIMER => return self.listen(),
            HELD_TIMER => return self.watch_held(),
            _ => {}
        }
        let Some(session) = &mut self.session else { return };
        match id {
            CARET_TIMER => {
                if let Some(typing) = &mut session.typing {
                    typing.caret_on = !typing.caret_on;
                    invalidate(window, typing.caret);
                    unsafe { SetTimer(window, CARET_TIMER, CARET_BLINK, None) };
                }
                return;
            }
            HINT_TIMER => session.show_hint = session.hovered.is_some(),
            RING_TIMER => session.ring = None,
            NOTE_TIMER => session.note = None,
            _ => return,
        }
        session.refresh(window);
    }

    fn key(&mut self, key: u16, repeated: bool) {
        let window = self.window;
        let (ctrl, shift) = (key_down(VK_CONTROL), key_down(VK_SHIFT));
        // While text is being written, letters go into it, and only a few keys do anything else:
        // the arrows, Home and End move the caret, Delete takes away the letter after it, Enter
        // puts the text down, Esc lets it go, Ctrl+C and Ctrl+S put it down first.
        if let Some(session) = &mut self.session
            && session.typing.is_some()
        {
            match key {
                VK_ESCAPE => return session.drop_text(window),
                VK_Z if ctrl => return session.drop_text(window),
                VK_RETURN if !shift => return session.put_down_text(window),
                VK_V if ctrl => return session.paste_text(window),
                VK_LEFT | VK_RIGHT | VK_HOME | VK_END | VK_DELETE => return session.text_key(window, key),
                VK_C | VK_S if ctrl => session.put_down_text(window),
                _ => return,
            }
        }
        let Some(session) = &self.session else { return };
        let action = match key {
            // Esc gives up a question asked aloud and keeps the screenshot.
            VK_ESCAPE if session.pressing.is_some() || session.listening.is_some() => Action::Hush,
            // While a screenshot is being sent, Esc cancels it and other keys wait.
            VK_ESCAPE if session.palette => Action::ClosePalette,
            VK_ESCAPE => Action::Close,
            _ if session.busy() => return,
            VK_RETURN => Action::Copy,
            VK_C if ctrl && shift => Action::CopyText,
            VK_C if ctrl => Action::Copy,
            VK_S if ctrl && shift => Action::SaveAs,
            VK_S if ctrl => Action::Save,
            VK_M if ctrl => Action::Mask,
            VK_Z if ctrl && shift => Action::Redo,
            VK_Z if ctrl => Action::Undo,
            VK_Y if ctrl => Action::Redo,
            VK_A if ctrl => Action::SelectMonitor,
            // Delete and Backspace remove the annotation picked.
            VK_DELETE | VK_BACK if session.picked.is_some() => Action::Remove,
            _ if ctrl => return,
            _ => match char::from_u32(key as u32).and_then(|c| session.button_for_key(c)) {
                // A button for a conversation, held, asks its question aloud.
                Some(Button::Query(i)) if session.converses(i) => match repeated {
                    false => Action::Hold(i, Holder::Key(key)),
                    true => return,
                },
                Some(button) => Action::Click(button),
                None => return,
            },
        };
        self.act(action);
    }

    fn act(&mut self, action: Action) {
        let window = self.window;
        match action {
            Action::Click(button) => self.click(button),
            Action::Hold(i, holder) => self.hold(i, holder),
            Action::Hush => self.hush(),
            Action::ClosePalette => {
                if let Some(session) = &mut self.session {
                    session.palette = false;
                    session.refresh(window);
                }
            }
            Action::Close => self.close(),
            Action::Copy => self.copy(),
            Action::CopyText => self.copy_text(),
            Action::Save => self.save(false),
            Action::SaveAs => self.save(true),
            Action::Mask => self.mask(),
            Action::Undo => self.change_history(Session::undo),
            Action::Redo => self.change_history(|session| session.history.redo()),
            Action::SelectMonitor => self.select_monitor(),
            Action::Remove => {
                if let Some(session) = &mut self.session {
                    session.remove_picked(window);
                }
            }
        }
    }

    fn click(&mut self, button: Button) {
        let window = self.window;
        match button {
            Button::Tool(tool) => {
                if let Some(session) = &mut self.session {
                    // A second click on the chosen tool puts it away, back to moving the selection.
                    session.tool = if session.tool == Some(tool) { None } else { Some(tool) };
                    session.refresh(window);
                }
            }
            Button::Color => {
                if let Some(session) = &mut self.session {
                    session.palette = !session.palette;
                    session.refresh(window);
                }
            }
            Button::Undo => self.change_history(Session::undo),
            Button::Mask => self.mask(),
            Button::Copy => self.copy(),
            Button::CopyText => self.copy_text(),
            Button::Save => self.save(false),
            Button::Query(i) => self.query(i),
            Button::Close => self.close(),
        }
    }

    /// A character typed: into the text being written, if there is one.
    fn typed(&mut self, character: u32) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        let Some(typing) = &mut session.typing else { return };
        let letter = match char::from_u32(character) {
            // Backspace takes away the letter before the caret.
            Some('\u{8}') => {
                if let Some((i, _)) = typing.text[..typing.position].char_indices().next_back() {
                    typing.text.remove(i);
                    typing.position = i;
                }
                None
            }
            // Enter puts the text down; with Shift it begins a new line.
            Some('\r') if key_down(VK_SHIFT) => Some('\n'),
            Some(c) if !c.is_control() => Some(c),
            _ => return,
        };
        if let Some(letter) = letter {
            typing.text.insert(typing.position, letter);
            typing.position += letter.len_utf8();
        }
        session.retype(window);
    }

    /// Undoes or redoes a change of the annotations, or starting over, and draws the selection
    /// anew.
    fn change_history(&mut self, change: fn(&mut Session) -> bool) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.draft.is_some() || session.grabbed.is_some() || !change(session) {
            return;
        }
        session.picked = None;
        session.layers = None;
        if let Some(selection) = session.selection.rect() {
            invalidate(window, selection);
        }
        session.refresh(window);
    }

    /// A double click on a label rewrites it, whatever the tool; elsewhere it is a click. Its
    /// first click has picked the label already, or started rewriting it with the text tool, or
    /// drawn a dot with a pencil, which is taken back.
    fn double_click(&mut self, p: Point) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.busy() || session.typing.is_some() {
            return;
        }
        let Some(index) = session.label_at(p) else { return self.press(p) };
        let pencil = matches!(session.tool, Some(Tool::Pencil | Tool::Brush | Tool::MagicPencil));
        let items = session.history.items();
        if let Some(dot) = items.last().filter(|_| pencil && index + 1 < items.len())
            && matches!(&dot.shape, Shape::Stroke(points) if points.len() == 1)
        {
            invalidate(window, dot.bounds());
            session.history.undo();
        }
        session.edit_label(window, index);
    }

    /// Copies the selection with its annotations into the clipboard and closes the overlay, as
    /// Ctrl+C does in Lightshot.
    fn copy(&mut self) {
        let Some(picture) = self.session.as_ref().and_then(Session::picture) else { return };
        match clipboard::put(self.window, &picture) {
            Ok(()) => self.close(),
            Err(error) => app::notify(cannot_copy(), &error.to_string()),
        }
    }

    /// Reads the text in the selection and copies it. The annotations are read with it, so that
    /// text hidden under a filled rectangle stays hidden. The overlay waits with a hint meanwhile.
    fn copy_text(&mut self) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.busy() {
            return;
        }
        let Some(picture) = session.picture() else { return };
        session.reading = Some(Button::CopyText);
        session.show_hint = false;
        session.palette = false;
        session.refresh(window);
        let target = window as isize;
        thread::spawn(move || {
            unsafe { CoInitializeEx(null(), COINIT_MULTITHREADED as u32) };
            let read = Box::into_raw(Box::new(ocr::read(&picture)));
            if unsafe { PostMessageW(target as HWND, WM_READ, 0, read as isize) } == 0 {
                drop(unsafe { Box::from_raw(read) });
            }
        });
    }

    /// The text of the screenshot has been read: it is copied and the overlay closes, or the
    /// toolbar says why not.
    fn text_read(&mut self, read: Read) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.reading != Some(Button::CopyText) {
            return;
        }
        session.reading = None;
        let note = match read {
            Ok(text) if !text.trim().is_empty() => match clipboard::put_text(window, &text) {
                Ok(()) => return self.close(),
                Err(error) => format!("{}: {error}", cannot_copy()),
            },
            Ok(_) => tr("No text found").to_string(),
            Err(problem) => problem,
        };
        session.set_note(window, Button::CopyText, note);
    }

    /// Covers the private data in the selection: its text is read, and the words of what the
    /// finder marks in it go under filled rectangles of the colour chosen, all in one step of
    /// Ctrl+Z. The overlay waits with a hint meanwhile.
    fn mask(&mut self) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.busy() {
            return;
        }
        let Some(selection) = session.selection.rect().filter(|r| !r.is_empty()) else { return };
        let Some(picture) = session.picture() else { return };
        session.reading = Some(Button::Mask);
        session.show_hint = false;
        session.palette = false;
        session.refresh(window);
        let target = window as isize;
        thread::spawn(move || {
            unsafe { CoInitializeEx(null(), COINIT_MULTITHREADED as u32) };
            let masked: Masked = masking::mask(&picture).map(|masking| {
                eprintln!("Masking:\n{}", masking.report);
                // For looking into masking that went wrong: what it read, found and covered.
                if let Some(folder) = std::env::var_os("SCREENSHOT_MASKING_KEEP") {
                    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                    let name = format!("mask-{}", since.as_millis());
                    if let Err(problem) = masking::keep(std::path::Path::new(&folder), &name, &picture, &masking) {
                        eprintln!("The masking wasn't kept: {problem}");
                    }
                }
                let covers = masking.covers.into_iter();
                covers.map(|rect| rect.offset(selection.left, selection.top).intersect(selection)).collect()
            });
            let masked = Box::into_raw(Box::new(masked));
            if unsafe { PostMessageW(target as HWND, WM_MASKED, 0, masked as isize) } == 0 {
                drop(unsafe { Box::from_raw(masked) });
            }
        });
    }

    /// The private data of the screenshot has been looked for: it is covered, or the toolbar
    /// says that there is none, or why it wasn't looked for.
    fn masked(&mut self, masked: Masked) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.reading != Some(Button::Mask) {
            return;
        }
        session.reading = None;
        // While the model is downloaded, only the rules have looked: the note says so.
        let coming = models::coming().map(|coming| {
            tr("Names and addresses aren't looked for until the model is downloaded: {}%").replace("{}", &percent(coming))
        });
        match masked {
            Ok(covers) if !covers.is_empty() => {
                session.cover(window, &covers);
                if let Some(coming) = coming {
                    session.set_note(window, Button::Mask, coming);
                }
            }
            Ok(_) => session.set_note(window, Button::Mask, coming.unwrap_or_else(|| tr("No private data found").into())),
            Err(problem) => session.set_note(window, Button::Mask, problem),
        }
    }

    /// Saves the selection with its annotations to a PNG file: straight into the folder from the
    /// settings, or where the user says, if `ask` or the settings ask where; and into the
    /// clipboard too, as the settings say.
    fn save(&mut self, ask: bool) {
        let window = self.window;
        let Some(session) = &self.session else { return };
        let Some(picture) = session.picture() else { return };
        let save = session.settings.save.clone();
        let folder = match save.folder.trim() {
            "" => shell::screenshots_folder(),
            folder => PathBuf::from(folder),
        };
        let name = shell::file_name(&save.name);
        let ask = ask || save.ask;
        let path = if ask {
            match shell::ask_where_to_save(window, &folder, &name) {
                Some(path) => path,
                None => return,
            }
        } else {
            if let Err(error) = fs::create_dir_all(&folder) {
                app::notify(cannot_save(), &format!("{}: {error}", folder.display()));
                return;
            }
            shell::new_path(&folder, &name)
        };
        match picture.to_png().and_then(|png| fs::write(&path, png).map_err(|e| e.to_string())) {
            Ok(()) => {
                // The screenshot goes into the clipboard too, if the settings say so. A problem
                // with that is what the user hears of; otherwise the notification says where the
                // file is, unless the user has just said it.
                match save.copy.then(|| clipboard::put(window, &picture)) {
                    Some(Err(error)) => app::notify(cannot_copy(), &error.to_string()),
                    copied if !ask => app::saved(&path, copied.is_some()),
                    _ => {}
                }
                self.close();
            }
            Err(error) => app::notify(cannot_save(), &format!("{}: {error}", path.display())),
        }
    }

    /// A button for a conversation is pressed: let go at once, it asks the question of the
    /// settings; held, it asks one aloud.
    fn hold(&mut self, button: usize, holder: Holder) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.busy() {
            return;
        }
        session.pressing = Some(Press { button, holder });
        session.note = None;
        session.show_hint = false;
        unsafe {
            SetTimer(window, HOLD_TIMER, HOLD_TIME, None);
            SetTimer(window, HELD_TIMER, HELD_TICK, None);
        }
        session.refresh(window);
    }

    /// A held button is let go: sooner than the time to ask aloud, it asks the question of the
    /// settings; while its question is asked aloud, the page finishes listening and sends it.
    fn let_go(&mut self, holder: Holder) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.pressing.as_ref().is_some_and(|press| press.holder == holder) {
            unsafe {
                KillTimer(window, HOLD_TIMER);
                KillTimer(window, HELD_TIMER);
            }
            if let Some(press) = session.pressing.take() {
                self.query(press.button);
            }
            return;
        }
        if let Some(listening) = session.listening.as_mut().filter(|l| l.holder == holder && !l.released) {
            listening.released = true;
            unsafe { KillTimer(window, HELD_TIMER) };
            browser::stop_listening();
            session.refresh(window);
        }
    }

    /// Looks whether the held button is still held, in case it was let go over another window.
    fn watch_held(&mut self) {
        let Some(session) = &self.session else { return };
        let pressed = session.pressing.as_ref().map(|press| press.holder);
        let listening = session.listening.as_ref().filter(|l| !l.released).map(|l| l.holder);
        let Some(holder) = pressed.or(listening) else { return };
        let key = match holder {
            Holder::Key(key) => key,
            // The left button is the right one when the mouse's buttons are swapped.
            Holder::Mouse if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 => VK_RBUTTON,
            Holder::Mouse => VK_LBUTTON,
        };
        if unsafe { GetAsyncKeyState(key as i32) } < 0 {
            unsafe { SetTimer(self.window, HELD_TIMER, HELD_TICK, None) };
        } else {
            self.let_go(holder);
        }
    }

    /// The button for a conversation has been held long enough: its question is asked aloud.
    /// The screenshot goes to AI Mode while the microphone goes on, and the frozen screen shows
    /// what is heard until the button is let go.
    fn listen(&mut self) {
        let window = self.window;
        self.queries += 1;
        let id = self.queries;
        let Some(session) = &mut self.session else { return };
        let Some(press) = session.pressing.take() else { return };
        let Some(picture) = session.picture() else { return };
        let upload = picture.clone();
        let (button, holder) = (press.button, press.holder);
        session.listening = Some(Listening { id, button, holder, picture, on: false, words: String::new(), released: false });
        session.refresh(window);
        let target = window as isize;
        thread::spawn(move || {
            let answer: Answer = upload.to_png().map(|png| query::listen(&png));
            let answer = Box::into_raw(Box::new(answer));
            if unsafe { PostMessageW(target as HWND, WM_ANSWER, id as usize, answer as isize) } == 0 {
                drop(unsafe { Box::from_raw(answer) });
            }
        });
    }

    /// The screenshot of a question asked aloud is ready: the window starts listening, hidden.
    fn start_listening(&mut self, answer: Answer) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        let Some(listening) = &session.listening else { return };
        let button = session.settings.buttons[listening.button].clone();
        let selection = session.selection.rect().map(|r| r.offset(session.screen.origin.x, session.screen.origin.y));
        let started = match answer {
            Ok(Way::Window { url, script }) => {
                let picture = listening.picture.clone();
                let job = browser::Job { name: button.name.clone(), script, picture, mode: button.mode, listener: Some(window) };
                browser::ask(&url, job, selection)
            }
            Ok(Way::Page(_)) => Err(tr("AI Mode opens only in the program's window.")
            .into()),
            Err(problem) => Err(problem),
        };
        match started {
            Ok(()) if listening.released => browser::stop_listening(),
            Ok(()) => {}
            Err(problem) => {
                let button = listening.button;
                session.listening = None;
                let failed = tr("Couldn't ask aloud:");
                session.set_note(window, Button::Query(button), format!("{failed} {problem}"));
            }
        }
    }

    /// What the page has heard of the question asked aloud.
    fn heard(&mut self, heard: Heard) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        let Some(listening) = &mut session.listening else { return };
        let button = listening.button;
        match heard {
            Heard::Listening => listening.on = true,
            Heard::Words(words) => listening.words = words,
            Heard::Sent => {
                session.listening = None;
                return self.close();
            }
            Heard::Silent => {
                session.listening = None;
                session.set_note(window, Button::Query(button), tr("Heard nothing").into());
            }
            Heard::Failed(why) => {
                session.listening = None;
                session.set_note(window, Button::Query(button), why);
            }
        }
        session.refresh(window);
    }

    /// Esc while a question is asked aloud: it is given up, and the screenshot stays.
    fn hush(&mut self) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        unsafe {
            KillTimer(window, HOLD_TIMER);
            KillTimer(window, HELD_TIMER);
        }
        session.pressing = None;
        if session.listening.take().is_some() {
            browser::abort_listening();
        }
        session.refresh(window);
    }

    /// Sends the selection with its annotations as the query button says. The overlay waits
    /// with a hint until the answer comes, then opens it in the browser and closes.
    fn query(&mut self, index: usize) {
        let window = self.window;
        self.queries += 1;
        let id = self.queries;
        let Some(session) = &mut self.session else { return };
        if session.sending.is_some() || session.listening.is_some() {
            return;
        }
        let Some(button) = session.settings.buttons.get(index).cloned() else { return };
        let Some(picture) = session.picture() else { return };
        let upload = picture.clone();
        session.sending = Some(Sending { id, button: index, picture });
        session.show_hint = false;
        session.palette = false;
        session.refresh(window);
        let target = window as isize;
        thread::spawn(move || {
            let answer: Answer = upload.to_png().map(|png| query::send(&button, &png));
            let answer = Box::into_raw(Box::new(answer));
            if unsafe { PostMessageW(target as HWND, WM_ANSWER, id as usize, answer as isize) } == 0 {
                drop(unsafe { Box::from_raw(answer) });
            }
        });
    }

    fn answered(&mut self, id: u64, answer: Answer) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        if session.listening.as_ref().map(|l| l.id) == Some(id) {
            return self.start_listening(answer);
        }
        if session.sending.as_ref().map(|s| s.id) != Some(id) {
            return;
        }
        let Some(sending) = session.sending.take() else { return };
        let button = session.settings.buttons[sending.button].clone();
        // When the screenshot can't go by itself, it goes to the clipboard and Google opens
        // with the question, so that Ctrl+V finishes the query.
        let paste = |url: &str, title: &str, why: &str| {
            let copied = clipboard::put(window, &sending.picture).is_ok();
            let _ = shell::open(url);
            let how = if copied { query::paste_hint(button.mode) } else { "" };
            app::notify(title, &format!("{why}{how}"));
        };
        match answer {
            Ok(Way::Page(page)) => {
                if let Err(problem) = shell::open_page(&page) {
                    let title = not_sent(&button.name);
                    paste(&query::fallback(&button), &title, &format!("{problem} "));
                }
            }
            Ok(Way::Window { url, script }) => {
                // The window, or the answer card, goes next to the selection, which is on the
                // desktop where the frozen screen showed it.
                let selection = session.selection.rect().map(|r| r.offset(session.screen.origin.x, session.screen.origin.y));
                if button.mode == Mode::Answer && let Some(selection) = selection {
                    card::open(selection);
                }
                let picture = sending.picture.clone();
                let job = browser::Job { name: button.name.clone(), script, picture, mode: button.mode, listener: None };
                if let Err(problem) = browser::ask(&url, job, selection) {
                    card::close();
                    let title = not_sent(&button.name);
                    paste(&query::fallback(&button), &title, &format!("{problem} "));
                }
            }
            Err(problem) => {
                let title = not_sent(&button.name);
                paste(&query::fallback(&button), &title, &format!("{problem} "));
            }
        }
        self.close();
    }

    /// Selects the whole monitor under the mouse.
    fn select_monitor(&mut self) {
        let window = self.window;
        let Some(session) = &mut self.session else { return };
        let monitor = session.screen.monitor_at(session.cursor()).rect;
        session.selection.select(monitor);
        session.refresh(window);
    }

    /// Hides the overlay and lets the frozen screen go; a question asked aloud is given up.
    fn close(&mut self) {
        let Some(session) = self.session.take() else { return };
        if session.listening.is_some() {
            browser::abort_listening();
        }
        self.color = session.color;
        self.width = Some(session.width);
        unsafe {
            for timer in [HINT_TIMER, RING_TIMER, HOLD_TIMER, HELD_TIMER, NOTE_TIMER, CARET_TIMER] {
                KillTimer(self.window, timer);
            }
            ReleaseCapture();
            ShowWindow(self.window, SW_HIDE);
        }
    }
}

impl Session {
    fn start(settings: Rc<Settings>, color: usize, width: Option<f32>) -> io::Result<Session> {
        Session::over(capture::capture()?, settings, color, width)
    }

    /// A screenshot of the screen frozen, with nothing selected yet.
    fn over(screen: Screen, settings: Rc<Settings>, color: usize, width: Option<f32>) -> io::Result<Session> {
        let buffer = Buffer::new(screen.image.width, screen.image.height)?;
        let mut colors = settings.palette();
        if colors.is_empty() {
            colors.push([229, 57, 53, 255]);
        }
        Ok(Session {
            selection: Selection::new(screen.image.bounds()),
            screen,
            buffer,
            color: color.min(colors.len() - 1),
            colors,
            width: width.unwrap_or(settings.width),
            settings,
            tool: None,
            history: History::default(),
            earlier: Vec::new(),
            draft: None,
            layers: None,
            draft_changed: false,
            hovered: None,
            show_hint: false,
            palette: false,
            ring: None,
            sending: None,
            pressing: None,
            listening: None,
            reading: None,
            note: None,
            typing: None,
            picked: None,
            grabbed: None,
            snap: None,
            snap_press: None,
            decorated: None,
            fonts: Vec::new(),
        })
    }

    /// Where the mouse is on the frozen screen.
    fn cursor(&self) -> Point {
        let mut at = POINT::default();
        unsafe { GetCursorPos(&mut at) };
        Point::new(at.x - self.screen.origin.x, at.y - self.screen.origin.y)
    }

    fn reach(&self, p: Point) -> i32 {
        draw::edge_reach(self.screen.monitor_at(p).scale)
    }

    /// The selection, while it is shown and isn't being drawn, moved or resized: then the
    /// toolbars are next to it.
    fn settled(&self) -> Option<Rect> {
        let selection = self.selection.rect().filter(|r| !r.is_empty())?;
        (!self.selection.is_dragging()).then_some(selection)
    }

    /// The main toolbar and that of the actions, where the settings put them around the selection.
    fn panels(&self) -> Option<(Toolbar, Toolbar)> {
        let selection = self.settled()?;
        let monitor_at = |p: Point| {
            let monitor = self.screen.monitor_at(p);
            (monitor.rect, monitor.scale)
        };
        Some(Toolbar::panels(selection, &self.settings.panels, &self.query_groups(), monitor_at))
    }

    /// The main toolbar.
    fn toolbar(&self) -> Option<Toolbar> {
        self.panels().map(|(main, _)| main)
    }

    /// The button of either toolbar under the point.
    fn button_at(&self, p: Point) -> Option<Button> {
        let (main, actions) = self.panels()?;
        main.hit(p).or_else(|| actions.hit(p))
    }

    /// The query buttons by their numbers in the settings, in two groups: those that ask the AI,
    /// then those that search by image.
    fn query_groups(&self) -> [Vec<usize>; 2] {
        let (ai, images) = (0..self.settings.buttons.len()).partition(|&i| self.settings.buttons[i].mode != Mode::Images);
        [ai, images]
    }

    fn palette_layout(&self) -> Option<Palette> {
        if !self.palette {
            return None;
        }
        Palette::layout(&self.toolbar()?, self.colors.len())
    }

    /// The cursor shows what the mouse would do: draw, move or resize the selection, press a
    /// button, or nothing.
    fn cursor_shape(&self) -> PCWSTR {
        if self.sending.is_some() || self.reading.is_some() {
            return IDC_APPSTARTING;
        }
        let p = self.cursor();
        let on_panel = self.panels().is_some_and(|(main, actions)| main.rect.contains(p) || actions.rect.contains(p))
            || self.palette_layout().is_some_and(|palette| palette.rect.contains(p));
        if on_panel {
            return IDC_ARROW;
        }
        // Over an annotation the mouse takes it: a hand for a handle, a cross of arrows for the
        // annotation whole, the text cursor for a label to rewrite.
        if let Some((_, grip)) = self.annotation_at(p) {
            return match grip {
                Grip::Handle(_) => IDC_HAND,
                Grip::Body if self.tool == Some(Tool::Text) => IDC_IBEAM,
                Grip::Body => IDC_SIZEALL,
            };
        }
        match (self.selection.hit(p, self.reach(p)), self.tool) {
            (Hit::Edges(e), _) => match (e.left || e.right, e.top || e.bottom) {
                (true, true) if e.left == e.top => IDC_SIZENWSE,
                (true, true) => IDC_SIZENESW,
                (true, false) => IDC_SIZEWE,
                _ => IDC_SIZENS,
            },
            (Hit::Inside, Some(Tool::Text)) => IDC_IBEAM,
            (Hit::Inside, Some(_)) => IDC_CROSS,
            (Hit::Inside, None) => IDC_SIZEALL,
            (Hit::Outside, _) if !self.history.is_empty() => IDC_ARROW,
            (Hit::Outside, _) => IDC_CROSS,
        }
    }

    /// The annotation under the point, inside the selection, and what of it the mouse takes: a
    /// handle of the one picked, with any tool; any annotation whole, with no tool chosen; a
    /// label, with the text tool. The edges of the selection go before annotations under them.
    fn annotation_at(&self, p: Point) -> Option<(usize, Grip)> {
        self.selection.rect().filter(|r| !r.is_empty() && r.contains(p))?;
        let at = PointF::new(p.x as f32 + 0.5, p.y as f32 + 0.5);
        let scale = self.screen.monitor_at(p).scale;
        if let Some(index) = self.picked
            && let Some(handle) = self.history.items().get(index).and_then(|a| a.handle_at(at, HANDLE_REACH * scale))
        {
            return Some((index, Grip::Handle(handle)));
        }
        if matches!(self.selection.hit(p, self.reach(p)), Hit::Edges(_)) {
            return None;
        }
        let index = self.touched(p, |a| match self.tool {
            None => true,
            Some(Tool::Text) => matches!(a.shape, Shape::Text(_)),
            Some(_) => false,
        })?;
        Some((index, Grip::Body))
    }

    /// The topmost annotation the point is on, of those `takes` accepts.
    fn touched(&self, p: Point, takes: impl Fn(&Annotation) -> bool) -> Option<usize> {
        let at = PointF::new(p.x as f32 + 0.5, p.y as f32 + 0.5);
        let reach = TOUCH_REACH * self.screen.monitor_at(p).scale;
        self.history.items().iter().rposition(|a| takes(a) && a.touches(at, reach))
    }

    /// The label under the point, inside the selection, the topmost one.
    fn label_at(&self, p: Point) -> Option<usize> {
        self.selection.rect().filter(|r| r.contains(p))?;
        self.touched(p, |a| matches!(a.shape, Shape::Text(_)))
    }

    /// Takes an annotation by a handle or whole, to drag it; it is picked, and the change is
    /// remembered for undo.
    fn grab(&mut self, window: HWND, index: usize, grip: Grip, p: Point) {
        let Some(original) = self.history.items().get(index).cloned() else { return };
        self.history.remember();
        self.picked = Some(index);
        let from = PointF::new(p.x as f32 + 0.5, p.y as f32 + 0.5);
        self.grabbed = Some(Grab { index, grip, from, original, moving: false });
        self.layers = None;
        self.refresh(window);
    }

    /// No annotation shows its handles.
    fn unpick(&mut self, window: HWND) {
        if self.picked.take().is_some() {
            self.refresh(window);
        }
    }

    /// Removes the annotation picked; undo brings it back.
    fn remove_picked(&mut self, window: HWND) {
        let Some(index) = self.picked.take() else { return };
        self.history.remember();
        if let Some(gone) = self.history.take(index) {
            invalidate(window, gone.bounds());
        }
        self.layers = None;
        self.refresh(window);
    }

    /// Starts the selection over on the same frozen screen: the window under the mouse lights up
    /// again, and a click or the mouse selects anew, inside the selection that was too. The text
    /// being written is put down first; the selection goes with its annotations and the tool,
    /// for undo to bring back.
    fn start_over(&mut self, window: HWND) {
        self.put_down_text(window);
        // An annotation being dragged stays where it has got to; if it hasn't moved, there is
        // nothing to undo.
        if self.grabbed.take().is_some() {
            self.history.forget_if_unchanged();
        }
        if let Some(selection) = self.selection.rect().filter(|r| !r.is_empty()) {
            let history = std::mem::take(&mut self.history);
            self.earlier.push(Earlier { selection, history, tool: self.tool });
        }
        self.selection.clear();
        self.history.clear();
        self.picked = None;
        self.draft = None;
        self.layers = None;
        self.tool = None;
        self.palette = false;
        self.hovered = None;
        self.show_hint = false;
        self.snap_press = None;
        self.snap = self.screen.window_at(self.cursor());
        self.refresh(window);
    }

    /// Takes back the last change of the annotations; with none left, starting over, which brings
    /// back the selection with its annotations and the tool. Whether there was anything to take
    /// back.
    fn undo(&mut self) -> bool {
        if self.history.undo() {
            return true;
        }
        if self.selection.is_dragging() {
            return false;
        }
        let Some(earlier) = self.earlier.pop() else { return false };
        self.selection.select(earlier.selection);
        self.history = earlier.history;
        self.tool = earlier.tool;
        self.snap = None;
        self.snap_press = None;
        true
    }

    /// Rewrites a label: it leaves the annotations while it is written and comes back to its
    /// place when put down, in its colour and size unless they are changed meanwhile; Esc brings
    /// it back as it was.
    fn edit_label(&mut self, window: HWND, index: usize) {
        let Some(original) = self.history.items().get(index).cloned() else { return };
        let Shape::Text(label) = &original.shape else { return };
        let room = annotation::text_margin(label.size);
        let scale = self.screen.monitor_at(label.at).scale;
        let at = Point::new(label.at.x + room, label.at.y + room);
        // The line width whose text has this size.
        let width = ((label.size / scale - annotation::text_size(0.0)) / 2.0).max(1.0);
        let text = label.text.clone();
        self.history.remember();
        self.history.take(index);
        self.picked = None;
        self.layers = None;
        invalidate(window, original.bounds());
        let color = original.color;
        self.typing = Some(Typing {
            at,
            position: text.len(),
            text,
            color,
            width,
            label: None,
            caret: Rect::default(),
            caret_on: true,
            editing: Some((index, original)),
        });
        input_method(window, true);
        self.retype(window);
    }

    /// Starts writing text where the selection was clicked, in the colour and size chosen.
    fn start_typing(&mut self, window: HWND, at: Point) {
        self.picked = None;
        self.typing = Some(Typing {
            at,
            text: String::new(),
            position: 0,
            color: self.colors[self.color],
            width: self.width,
            label: None,
            caret: Rect::default(),
            caret_on: true,
            editing: None,
        });
        input_method(window, true);
        self.retype(window);
    }

    /// Draws the text being written again after it, its colour or its size has changed: its
    /// letters in the size for its line width, at the monitor's scale, and the caret after them.
    fn retype(&mut self, window: HWND) {
        let Some(typing) = &mut self.typing else { return };
        let (color, width) = (typing.color, typing.width);
        let scale = self.screen.monitor_at(typing.at).scale;
        let size = annotation::text_size(width) * scale;
        let room = annotation::text_margin(size);
        let before = typing.label.as_ref().map_or(Rect::default(), Annotation::bounds).union(typing.caret);
        match gdi::letters(&typing.text, size.round() as i32, room, typing.position) {
            Ok((coverage, (x, y, height))) => {
                let at = Point::new(typing.at.x - room, typing.at.y - room);
                let bar = (1.5 * scale).round().max(1.0) as i32;
                typing.caret = Rect::new(at.x + x, at.y + y, at.x + x + bar, at.y + y + height);
                compose_at(window, typing.caret);
                typing.label = (!typing.text.is_empty()).then(|| Annotation {
                    shape: Shape::Text(Label { text: typing.text.clone(), size, at, coverage }),
                    color,
                    width: width * scale,
                });
            }
            Err(error) => eprintln!("The label couldn't be drawn: {error}"),
        }
        typing.caret_on = true;
        let after = typing.label.as_ref().map_or(Rect::default(), Annotation::bounds).union(typing.caret);
        self.draft_changed = true;
        invalidate(window, before.union(after));
        unsafe { SetTimer(window, CARET_TIMER, CARET_BLINK, None) };
    }

    /// Puts the text being written down on the selection, like any other annotation, and picks
    /// it; a label rewritten goes back to its place among the others. Empty text is let go, and
    /// a label rewritten empty is removed.
    fn put_down_text(&mut self, window: HWND) {
        let Some(typing) = self.typing.take() else { return };
        input_method(window, false);
        unsafe { KillTimer(window, CARET_TIMER) };
        invalidate(window, typing.caret);
        let label = typing.label.filter(|_| !typing.text.trim().is_empty());
        if let Some(label) = &label {
            invalidate(window, label.bounds());
        }
        match (typing.editing, label) {
            (Some((index, _)), Some(label)) => {
                self.history.insert(index, label);
                self.history.forget_if_unchanged();
                self.picked = Some(index);
            }
            (None, Some(label)) => {
                self.history.push(label);
                self.picked = Some(self.history.items().len() - 1);
            }
            (_, None) => {}
        }
        self.layers = None;
        self.refresh(window);
    }

    /// Lets the text being written go; a label being rewritten comes back as it was.
    fn drop_text(&mut self, window: HWND) {
        let Some(typing) = self.typing.take() else { return };
        input_method(window, false);
        unsafe { KillTimer(window, CARET_TIMER) };
        invalidate(window, typing.label.as_ref().map_or(Rect::default(), Annotation::bounds).union(typing.caret));
        if let Some((index, original)) = typing.editing {
            invalidate(window, original.bounds());
            self.history.insert(index, original);
            self.history.forget_if_unchanged();
            self.layers = None;
        }
        self.draft_changed = true;
        self.refresh(window);
    }

    /// Covers parts of the screen with filled rectangles of the colour chosen, as one change.
    fn cover(&mut self, window: HWND, covers: &[Rect]) {
        let color = self.colors[self.color];
        self.history.remember();
        for &rect in covers {
            let scale = self.screen.monitor_at(rect.top_left()).scale;
            let (a, b) = (PointF::new(rect.left as f32, rect.top as f32), PointF::new(rect.right as f32, rect.bottom as f32));
            let annotation = Annotation { shape: Shape::FilledRectangle(a, b), color, width: self.width * scale };
            invalidate(window, annotation.bounds());
            let end = self.history.items().len();
            self.history.insert(end, annotation);
        }
        self.picked = None;
        self.layers = None;
        self.refresh(window);
    }

    /// Puts the text in the clipboard into the text being written, at the caret.
    fn paste_text(&mut self, window: HWND) {
        let Some(typing) = &mut self.typing else { return };
        let Some(pasted) = clipboard::text(window) else { return };
        let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
        typing.text.insert_str(typing.position, &pasted);
        typing.position += pasted.len();
        self.retype(window);
    }

    /// Moves the caret of the text being written by a letter, to the start or the end of its
    /// line, or takes away the letter after it.
    fn text_key(&mut self, window: HWND, key: u16) {
        let Some(typing) = &mut self.typing else { return };
        let (text, at) = (&typing.text, typing.position);
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
        match key {
            VK_LEFT => typing.position = text[..at].char_indices().next_back().map_or(0, |(i, _)| i),
            VK_RIGHT => typing.position = text[at..].chars().next().map_or(at, |c| at + c.len_utf8()),
            VK_HOME => typing.position = line_start,
            VK_END => typing.position = line_end,
            VK_DELETE if at < text.len() => {
                typing.text.remove(at);
            }
            _ => return,
        }
        self.retype(window);
    }

    /// Whether a query is on its way or a button is held: other keys and clicks wait.
    fn busy(&self) -> bool {
        self.sending.is_some() || self.pressing.is_some() || self.listening.is_some() || self.reading.is_some()
    }

    /// Whether the query button asks for a conversation: held, it asks its question aloud.
    fn converses(&self, button: usize) -> bool {
        self.settings.buttons.get(button).is_some_and(|b| b.mode == Mode::Chat)
    }

    /// Shows a note next to the query button for a few seconds.
    fn set_note(&mut self, window: HWND, button: Button, note: String) {
        self.note = Some((button, note));
        unsafe { SetTimer(window, NOTE_TIMER, NOTE_TIME, None) };
        self.refresh(window);
    }

    /// What the hint of a question asked aloud says: that the microphone goes on, what to do,
    /// what has been heard, and that the last words are being waited for.
    fn listening_hint(&self, listening: &Listening) -> String {
        let shown = || {
            let words: Vec<char> = listening.words.chars().collect();
            match words.len() > WORDS_SHOWN {
                true => format!("…{}", words[words.len() - WORDS_SHOWN..].iter().collect::<String>()),
                false => listening.words.clone(),
            }
        };
        let letting_go = match listening.holder {
            Holder::Key(_) => self.settings.buttons[listening.button]
                .key()
                .map(|key| tr("let go of {key}").replace("{key}", &key.to_string())),
            Holder::Mouse => None,
        };
        match (listening.on, listening.released, listening.words.is_empty()) {
            (false, false, _) => tr("Turning the microphone on…").into(),
            (_, true, true) => tr("Listening to the end…").into(),
            (_, true, false) => format!("{} {}", tr("Asking:"), shown()),
            (true, false, true) => {
                let letting_go = letting_go.unwrap_or_else(|| tr("let go of the button").into());
                tr("Speak · {} to ask").replace("{}", &letting_go)
            }
            (true, false, false) => shown(),
        }
    }

    /// The button a letter or digit stands for: a tool, or a query button from the settings.
    fn button_for_key(&self, key: char) -> Option<Button> {
        let key = key.to_ascii_uppercase();
        let tool = Tool::ALL.into_iter().find(|&tool| tool.key() == key);
        tool.map(Button::Tool).or_else(|| self.settings.buttons.iter().position(|b| b.key() == Some(key)).map(Button::Query))
    }

    fn hint_text(&self, button: Button) -> String {
        match button {
            Button::Tool(tool) => format!("{} · {}", tool.name(), tool.key()),
            Button::Color => {
                tr("Colour · the width with the mouse wheel").into()
            }
            Button::Undo => tr("Undo · Ctrl+Z").into(),
            Button::Mask => match models::coming() {
                Some(coming) => tr("Mask private data · Ctrl+M · names and addresses once the model is downloaded, {}%")
                    .replace("{}", &percent(coming)),
                None => tr("Mask private data · Ctrl+M").into(),
            },
            Button::Copy => tr("Copy · Ctrl+C").into(),
            Button::CopyText => {
                tr("Copy the text · Ctrl+Shift+C").into()
            }
            // Ctrl+Shift+S asks where to save when the button doesn't.
            Button::Save => match self.settings.save.ask {
                true => tr("Save · Ctrl+S").into(),
                false => tr("Save · Ctrl+S · choose where · Ctrl+Shift+S").into(),
            },
            Button::Query(i) => {
                let button = &self.settings.buttons[i];
                match button.key() {
                    Some(key) => format!("{} · {key}", button.name),
                    None => button.name.clone(),
                }
            }
            Button::Close => tr("Close · Esc").into(),
        }
    }

    fn scene(&mut self) -> Scene {
        // A new selection that is still a click shows the window it would select; once the
        // mouse goes further, the window goes dark and the selection is drawn.
        let clicking = self.snap_press.filter(|_| {
            self.selection.is_dragging() && self.selection.rect().is_none_or(|r| r.width() < CLICK && r.height() < CLICK)
        });
        let selection = self.selection.rect().filter(|r| !r.is_empty() && clicking.is_none());
        let scale = selection.map_or(1.0, |s| self.screen.monitor_at(s.top_left()).scale);
        let (toolbar, actions) = self.panels().map_or((None, None), |(main, actions)| (Some(main), Some(actions)));
        let panels: Vec<Rect> = toolbar.iter().chain(&actions).map(|bar| bar.rect).collect();
        let label = selection.and_then(|s| self.label(s, scale, &panels));
        let palette = self.palette_layout();
        let hint = self.hint(toolbar.as_ref(), actions.as_ref());
        let ring = self.ring.map(|at| {
            (at, annotation::drawn_width(self.tool, self.width) * self.screen.monitor_at(at).scale)
        });
        let snap = clicking
            .or(self.snap.filter(|_| selection.is_none() && !self.selection.is_dragging()))
            .map(|snap| (snap, self.screen.monitor_at(snap.top_left()).scale));
        let picked = self.picked.filter(|_| selection.is_some() && self.typing.is_none()).and_then(|index| {
            let item = self.history.items().get(index)?;
            Some((index, draw::picked_bounds(item, scale)))
        });
        Scene { selection, scale, snap, picked, label, toolbar, actions, palette, hint, ring }
    }

    /// The selection's size next to it, clear of the panels.
    fn label(&mut self, selection: Rect, scale: f32, panels: &[Rect]) -> Option<(Rect, Text)> {
        let text = format!("{} × {}", selection.width(), selection.height());
        let size = draw::label_font(scale);
        let measured = self.measure(&text, size)?;
        let monitor = self.screen.monitor_at(selection.top_left()).rect;
        let rect = draw::label_place(selection, draw::pill_size(measured, scale), monitor, scale, panels);
        Some((rect, Text { text, rect, size, bold: false, color: draw::TEXT }))
    }

    /// The hint of the button under the mouse, what is being sent, what is heard of a question
    /// asked aloud, or what happened to it, next to its toolbar.
    fn hint(&mut self, main: Option<&Toolbar>, actions: Option<&Toolbar>) -> Option<(Rect, Text)> {
        let (button, text) = match (&self.sending, &self.listening, &self.note) {
            _ if self.reading == Some(Button::Mask) => (Button::Mask, tr("Looking for private data…").to_string()),
            _ if self.reading.is_some() => (Button::CopyText, tr("Reading the text…").to_string()),
            (Some(sending), _, _) => {
                (Button::Query(sending.button), tr("Sending to Google…").to_string())
            }
            (_, Some(listening), _) => (Button::Query(listening.button), self.listening_hint(listening)),
            (_, _, Some((button, note))) => (*button, note.clone()),
            _ if self.show_hint && !self.palette && self.pressing.is_none() => {
                let button = self.hovered?;
                (button, self.hint_text(button))
            }
            _ => return None,
        };
        let (toolbar, anchor) = [main, actions].into_iter().flatten().find_map(|bar| Some((bar, bar.button(button)?)))?;
        let size = draw::label_font(toolbar.scale);
        let measured = self.measure(&text, size)?;
        let rect = toolbar.popup(anchor, draw::pill_size(measured, toolbar.scale));
        Some((rect, Text { text, rect, size, bold: false, color: draw::TEXT }))
    }

    /// Marks for redrawing what changed around the selection since it was last drawn.
    fn refresh(&mut self, window: HWND) {
        let scene = self.scene();
        let now = scene.bounds();
        let changed = match (self.decorated, now) {
            (Some(before), Some(now)) => Some(before.union(now)),
            (before, now) => before.or(now),
        };
        self.decorated = now;
        if let Some(changed) = changed {
            invalidate(window, changed);
        }
    }

    /// Composes `area` of the overlay in the buffer.
    fn render(&mut self, area: Rect) {
        let scene = self.scene();
        let annotated = !self.history.is_empty() || self.draft.is_some() || self.typing.is_some();
        if annotated {
            self.update_layers();
        }
        let queries: Vec<Icon> = self.settings.buttons.iter().map(QueryButton::icon).collect();
        let mut texts = Vec::new();
        let mut frame = self.buffer.frame();
        draw::background(&mut frame, &self.screen.image, scene.selection.or(scene.snap.map(|(snap, _)| snap)), area);
        if let Some((snap, scale)) = scene.snap {
            draw::window_frame(&mut frame, snap, scale);
        }
        if let (true, Some(selection), Some(layers)) = (annotated, scene.selection, &self.layers) {
            draw::blend(frame.pixels, frame.width, &layers.current, layers.origin, area.intersect(selection));
        }
        if let (Some(typing), Some(selection)) = (&self.typing, scene.selection)
            && typing.caret_on
        {
            draw::caret(&mut frame, typing.caret.intersect(selection), typing.color);
        }
        if let Some(item) = scene.picked.and_then(|(index, _)| self.history.items().get(index)) {
            draw::picked(&mut frame, item, scene.scale);
        }
        if let Some(selection) = scene.selection {
            draw::selection_frame(&mut frame, selection, scene.scale);
        }
        if let Some((rect, text)) = scene.label {
            draw::pill(&mut frame, rect);
            texts.push(text);
        }
        let look = ToolbarLook {
            tool: self.tool,
            hovered: self.hovered,
            color: self.colors[self.color],
            width: self.width,
            can_undo: !self.history.is_empty() || !self.earlier.is_empty(),
            busy: self.sending.as_ref().map(|s| s.button).or(self.listening.as_ref().map(|l| l.button)),
            queries: &queries,
            coming: models::coming(),
        };
        if let Some(actions) = &scene.actions {
            texts.extend(draw::toolbar(&mut frame, actions, &look));
        }
        if let Some(toolbar) = &scene.toolbar {
            texts.extend(draw::toolbar(&mut frame, toolbar, &look));
            if let Some(palette) = &scene.palette {
                draw::palette(&mut frame, palette, &self.colors, self.color, toolbar.scale);
            }
        }
        if let Some((rect, text)) = scene.hint {
            draw::pill(&mut frame, rect);
            texts.push(text);
        }
        if let Some((at, diameter)) = scene.ring {
            draw::width_ring(&mut frame, at, diameter);
        }
        for text in &texts {
            self.draw_text(text);
        }
    }

    /// Makes the layers with the annotations fit the selection and show the one being drawn or
    /// dragged.
    fn update_layers(&mut self) {
        let Some(selection) = self.selection.rect().filter(|r| !r.is_empty()) else { return };
        let size = (selection.width() as u32, selection.height() as u32);
        let origin = selection.top_left();
        let items = self.history.items();
        let grabbed = self.grabbed.as_ref().map(|grab| grab.index).filter(|&i| i < items.len());
        let fits = self.layers.as_ref().is_some_and(|layers| {
            layers.origin == origin && (layers.finished.width(), layers.finished.height()) == size
        });
        if !fits {
            let (under, over) = match grabbed {
                Some(i) => (&items[..i], &items[i + 1..]),
                None => (items, &[][..]),
            };
            let Some(mut finished) = Pixmap::new(size.0, size.1) else { return };
            draw::annotations(&mut finished, under, origin);
            let above = match over.is_empty() {
                true => None,
                false => Pixmap::new(size.0, size.1).map(|mut above| {
                    draw::annotations(&mut above, over, origin);
                    above
                }),
            };
            self.layers = Some(Layers { origin, current: finished.clone(), finished, above });
            self.draft_changed = true;
        }
        if self.draft_changed
            && let Some(layers) = &mut self.layers
        {
            layers.current.data_mut().copy_from_slice(layers.finished.data());
            if let Some(item) = grabbed.and_then(|i| self.history.items().get(i)) {
                draw::annotations(&mut layers.current, [item], layers.origin);
            }
            if let Some(above) = &layers.above {
                draw::lay_over(&mut layers.current, above);
            }
            if let Some(draft) = &self.draft {
                draw::annotations(&mut layers.current, [draft.annotation()], layers.origin);
            }
            if let Some(label) = self.typing.as_ref().and_then(|typing| typing.label.as_ref()) {
                draw::annotations(&mut layers.current, [label], layers.origin);
            }
            self.draft_changed = false;
        }
    }

    /// The selection with its annotations, as it is copied, saved and sent.
    fn picture(&self) -> Option<Image> {
        let selection = self.selection.rect().filter(|r| !r.is_empty())?;
        let mut picture = self.screen.image.crop(selection);
        if !self.history.is_empty() {
            let mut layer = Pixmap::new(picture.width, picture.height)?;
            draw::annotations(&mut layer, self.history.items(), selection.top_left());
            let bounds = picture.bounds();
            draw::blend(&mut picture.pixels, picture.width, &layer, Point::new(0, 0), bounds);
        }
        Some(picture)
    }

    fn font(&mut self, size: i32, bold: bool) -> Option<&Font> {
        if let Some(i) = self.fonts.iter().position(|(key, _)| *key == (size, bold)) {
            return Some(&self.fonts[i].1);
        }
        match Font::new(size, bold) {
            Ok(font) => {
                self.fonts.push(((size, bold), font));
                self.fonts.last().map(|(_, font)| font)
            }
            Err(error) => {
                eprintln!("Could not create the font: {error}");
                None
            }
        }
    }

    fn measure(&mut self, text: &str, size: i32) -> Option<(i32, i32)> {
        let dc = self.buffer.dc();
        let text: Vec<u16> = text.encode_utf16().collect();
        Some(gdi::text_size(dc, self.font(size, false)?, &text))
    }

    fn draw_text(&mut self, text: &Text) {
        let dc = self.buffer.dc();
        let wide: Vec<u16> = text.text.encode_utf16().collect();
        let Some(font) = self.font(text.size, text.bold) else { return };
        let (width, height) = gdi::text_size(dc, font, &wide);
        let x = text.rect.left + (text.rect.width() - width) / 2;
        let y = text.rect.top + (text.rect.height() - height) / 2;
        gdi::draw_text(dc, font, &wide, x, y, text.color);
    }
}

impl Scene {
    /// The part of the overlay that everything around the selection covers.
    fn bounds(&self) -> Option<Rect> {
        let mut all = Rect::default();
        if let Some(selection) = self.selection {
            all = all.union(draw::selection_bounds(selection, self.scale));
        }
        // The window lit up is drawn bright as a whole.
        if let Some((snap, _)) = self.snap {
            all = all.union(snap);
        }
        if let Some((_, rect)) = self.picked {
            all = all.union(rect);
        }
        if let Some((rect, _)) = &self.label {
            all = all.union(*rect);
        }
        if let Some(toolbar) = &self.toolbar {
            let shadow = draw::shadow_reach(toolbar.scale);
            all = all.union(toolbar.rect.inflate(shadow));
            if let Some(palette) = &self.palette {
                all = all.union(palette.rect.inflate(shadow));
            }
        }
        if let Some(actions) = &self.actions {
            all = all.union(actions.rect.inflate(draw::shadow_reach(actions.scale)));
        }
        if let Some((rect, _)) = &self.hint {
            all = all.union(*rect);
        }
        if let Some((at, diameter)) = self.ring {
            all = all.union(draw::ring_bounds(at, diameter));
        }
        (!all.is_empty()).then_some(all)
    }
}

fn cannot_copy() -> &'static str {
    tr("Couldn't copy")
}

fn cannot_save() -> &'static str {
    tr("Couldn't save")
}

/// The title of the notification about a query whose picture didn't go.
fn not_sent(button: &str) -> String {
    tr("{}: the picture didn't go").replace("{}", button)
}

fn key_down(key: u16) -> bool {
    unsafe { GetKeyState(key as i32) < 0 }
}

/// A share from 0 to 1 in whole percents.
fn percent(share: f32) -> String {
    ((share * 100.0).floor() as i32).clamp(0, 99).to_string()
}

/// The model of masking has come further, or is in place: the toolbar of the frozen screen shows it.
pub fn models_changed() {
    OVERLAY.with(|cell| {
        if let Ok(mut overlay) = cell.try_borrow_mut()
            && let Some(overlay) = overlay.as_mut()
        {
            let window = overlay.window;
            if let Some(session) = &mut overlay.session {
                session.refresh(window);
            }
        }
    });
}

/// Turns the input method of the window on or off. An input method for Chinese, Japanese or
/// Korean takes the letters typed for itself, and the keys of the tools and buttons would do
/// nothing; so the overlay has one only while text is written, in whatever language.
fn input_method(window: HWND, on: bool) {
    unsafe { ImmAssociateContextEx(window, null_mut(), if on { IACE_DEFAULT } else { 0 }) };
}

/// Shows what the input method is composing at the caret of the text being written.
fn compose_at(window: HWND, caret: Rect) {
    unsafe {
        let context = ImmGetContext(window);
        if context.is_null() {
            return;
        }
        let form = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT { x: caret.left, y: caret.top },
            ..Default::default()
        };
        ImmSetCompositionWindow(context, &form);
        ImmReleaseContext(window, context);
    }
}

/// The point under the mouse as a point to draw at, kept inside the selection. Lines go
/// through the middles of pixels, so that thin ones stay sharp.
fn inside(p: Point, selection: Rect) -> PointF {
    PointF::new(
        (p.x as f32 + 0.5).clamp(selection.left as f32, selection.right as f32),
        (p.y as f32 + 0.5).clamp(selection.top as f32, selection.bottom as f32),
    )
}

fn invalidate(window: HWND, rect: Rect) {
    if !rect.is_empty() {
        unsafe { InvalidateRect(window, &to_win(rect), 0) };
    }
}

/// The point of a mouse message: two signed 16-bit halves of its parameter.
fn point(lparam: LPARAM) -> Point {
    Point::new((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32)
}

fn to_win(r: Rect) -> RECT {
    RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

fn from_win(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right, r.bottom)
}

/// Turns an attribute of a window in the desktop window manager on or off.
fn set_flag(window: HWND, attribute: DWMWINDOWATTRIBUTE, on: bool) {
    let value = BOOL::from(on);
    unsafe {
        DwmSetWindowAttribute(window, attribute as u32, (&value as *const BOOL).cast(), size_of::<BOOL>() as u32);
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;

    use super::*;

    /// Draws the overlay over a made-up screen into the PNG file named by `SCREENSHOT_PREVIEW`,
    /// to look at the panels and the annotations without running the program, with the hint of
    /// the masking button in the language of the tests:
    /// `SCREENSHOT_PREVIEW=preview.png cargo test -p ainitshot preview -- --ignored`, with
    /// `SCREENSHOT_PANELS='tools = { side = "left", align = "top" }'` for the panels elsewhere
    /// and `SCREENSHOT_PALETTE=1` for the colours open in place of the hint.
    #[test]
    #[ignore]
    fn preview() {
        let (width, height) = (1100, 420);
        let mut pixels = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let shade = 200 + ((x / 40 + y / 40) % 2 * 30) as u8;
                pixels.extend_from_slice(&[shade, shade - 20, shade - 60, 255]);
            }
        }
        let screen = Image { width: width as u32, height: height as u32, pixels };
        // The letters are drawn with GDI, as the overlay draws them, into the same pixels.
        let mut buffer = Buffer::new(width as u32, height as u32).unwrap();
        let mut frame = buffer.frame();
        let selection = Rect::new(120, 60, 780, 330);
        draw::background(&mut frame, &screen, Some(selection), screen.bounds());

        let red = [229, 57, 53, 255];
        let mut layer = Pixmap::new(selection.width() as u32, selection.height() as u32).unwrap();
        let mut drawn = Vec::new();
        for (tool, points) in [
            (Tool::Arrow, vec![(160.0, 290.0), (330.0, 150.0)]),
            (Tool::Rectangle, vec![(380.0, 100.0), (520.0, 190.0)]),
            (Tool::FilledRectangle, vec![(560.0, 230.0), (740.0, 290.0)]),
            (Tool::Brush, vec![(380.0, 260.0), (430.0, 262.0), (520.0, 258.0)]),
            (Tool::Pencil, vec![(560.0, 120.0), (600.0, 90.0), (650.0, 140.0), (700.0, 100.0)]),
        ] {
            let mut draft = Draft::start(tool, PointF::new(points[0].0, points[0].1), red, 4.0).unwrap();
            for &(x, y) in &points[1..] {
                draft.extend(PointF::new(x, y), false);
            }
            drawn.extend(draft.finish(false));
        }
        draw::annotations(&mut layer, &drawn, selection.top_left());
        draw::blend(frame.pixels, frame.width, &layer, selection.top_left(), selection);
        // The arrow picked shows its handles; the pencil's line, which only moves, its frame.
        draw::picked(&mut frame, &drawn[0], 1.0);
        draw::picked(&mut frame, &drawn[4], 1.0);
        draw::selection_frame(&mut frame, selection, 1.0);

        // The panels stand where `SCREENSHOT_PANELS` puts them, as the [panels] of the settings
        // file do, or where they stand by default.
        let panels: crate::settings::Panels = std::env::var("SCREENSHOT_PANELS")
            .map(|text| toml::from_str(&text).expect("SCREENSHOT_PANELS reads as [panels]"))
            .unwrap_or_default();
        let (bar, column) = Toolbar::panels(selection, &panels, &[vec![0, 1], vec![2]], |_| (screen.bounds(), 1.0));
        let queries = [Icon::Google, Icon::Translate, Icon::Images];
        let open = std::env::var_os("SCREENSHOT_PALETTE").is_some();
        let look = ToolbarLook {
            tool: Some(Tool::Arrow),
            hovered: (!open).then_some(Button::Mask),
            color: red,
            width: 4.0,
            can_undo: true,
            busy: None,
            queries: &queries,
            coming: None,
        };
        let mut texts = draw::toolbar(&mut frame, &bar, &look);
        texts.extend(draw::toolbar(&mut frame, &column, &look));
        let colors = Palette::layout(&bar, 8).filter(|_| open);
        if let Some(colors) = &colors {
            draw::palette(&mut frame, colors, &[red, [251, 140, 0, 255], [253, 216, 53, 255], [67, 160, 71, 255], [30, 136, 229, 255], [142, 36, 170, 255], [33, 33, 33, 255], [255, 255, 255, 255]], 0, 1.0);
        }

        // The hint of the button under the mouse, next to its panel, as the overlay puts it.
        let font = |size: i32, bold: bool| Font::new(size, bold).unwrap();
        let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        if colors.is_none() {
            let text = tr("Mask private data · Ctrl+M").to_string();
            let size = draw::label_font(1.0);
            let measured = gdi::text_size(buffer.dc(), &font(size, false), &wide(&text));
            let rect = column.popup(column.button(Button::Mask).unwrap(), draw::pill_size(measured, 1.0));
            draw::pill(&mut buffer.frame(), rect);
            texts.push(Text { text, rect, size, bold: false, color: draw::TEXT });
        }
        for text in &texts {
            let (font, letters) = (font(text.size, text.bold), wide(&text.text));
            let (w, h) = gdi::text_size(buffer.dc(), &font, &letters);
            let (x, y) = (text.rect.left + (text.rect.width() - w) / 2, text.rect.top + (text.rect.height() - h) / 2);
            gdi::draw_text(buffer.dc(), &font, &letters, x, y, text.color);
        }

        let path = std::env::var("SCREENSHOT_PREVIEW").expect("SCREENSHOT_PREVIEW names the file to write");
        let picture = Image { width: width as u32, height: height as u32, pixels: buffer.frame().pixels.to_vec() };
        std::fs::write(path, picture.to_png().unwrap()).unwrap();
    }

    /// The hotkey pressed again starts the selection over on the same frozen screen: a window
    /// selected whole gives way to a smaller part of it. Undo brings the window back with what
    /// was drawn on it and the tool, and so it does after a right click, which starts over too.
    #[test]
    fn the_hotkey_again_starts_the_selection_over_and_undo_brings_it_back() {
        // A hidden window takes what the overlay would draw anew.
        let window =
            unsafe { CreateWindowExW(0, w!("STATIC"), w!(""), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), null_mut(), null()) };
        assert!(!window.is_null());
        let big = Rect::new(100, 100, 700, 500);
        let (width, height) = (800, 600);
        let screen = Screen {
            origin: Point::new(0, 0),
            image: Image { width, height, pixels: vec![255; (width * height * 4) as usize] },
            monitors: vec![capture::Monitor { rect: Rect::new(0, 0, 800, 600), scale: 1.0 }],
            windows: vec![big],
        };
        let session = Session::over(screen, Rc::new(Settings::default()), 0, None).unwrap();
        let mut overlay = Overlay { window, session: Some(session), color: 0, width: None, queries: 0 };
        let drag = |overlay: &mut Overlay, from: (i32, i32), to: (i32, i32)| {
            overlay.press(Point::new(from.0, from.1));
            overlay.mouse_move(Point::new(to.0, to.1));
            overlay.release();
        };
        let state = |overlay: &Overlay| {
            let session = overlay.session.as_ref().unwrap();
            (session.selection.rect(), session.history.items().len(), session.tool)
        };

        // A click selects the window under it whole, and an arrow is drawn on it.
        drag(&mut overlay, (400, 300), (400, 300));
        overlay.click(Button::Tool(Tool::Arrow));
        drag(&mut overlay, (200, 200), (300, 250));
        assert_eq!(state(&overlay), (Some(big), 1, Some(Tool::Arrow)));

        // The hotkey again: nothing is selected, on the same frozen screen.
        let frozen = overlay.session.as_ref().unwrap().screen.image.pixels.as_ptr();
        OVERLAY.with(|cell| *cell.borrow_mut() = Some(overlay));
        open();
        let mut overlay = OVERLAY.with(|cell| cell.borrow_mut().take()).unwrap();
        assert_eq!(overlay.session.as_ref().unwrap().screen.image.pixels.as_ptr(), frozen);
        assert_eq!(state(&overlay), (None, 0, None));

        // The mouse selects a smaller part inside the window that was selected.
        drag(&mut overlay, (200, 200), (400, 300));
        assert_eq!(state(&overlay), (Some(Rect::new(200, 200, 400, 300)), 0, None));

        // Undo brings the window back with the arrow and the tool; the next undo takes the arrow
        // back; there is nothing more.
        overlay.act(Action::Undo);
        assert_eq!(state(&overlay), (Some(big), 1, Some(Tool::Arrow)));
        overlay.act(Action::Undo);
        assert_eq!(state(&overlay), (Some(big), 0, Some(Tool::Arrow)));
        overlay.act(Action::Undo);
        assert_eq!(state(&overlay), (Some(big), 0, Some(Tool::Arrow)));

        // A right click starts over as well; undo brings the window back, and redo the arrow.
        overlay.right_click();
        assert_eq!(state(&overlay), (None, 0, None));
        overlay.act(Action::Undo);
        overlay.act(Action::Redo);
        assert_eq!(state(&overlay), (Some(big), 1, Some(Tool::Arrow)));

        drop(overlay);
        unsafe { DestroyWindow(window) };
    }

    /// A window, hidden, gives up its input method and takes it back.
    #[test]
    fn the_input_method_goes_and_comes_back() {
        let window =
            unsafe { CreateWindowExW(0, w!("STATIC"), w!(""), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), null_mut(), null()) };
        assert!(!window.is_null());
        let has_one = || unsafe {
            let context = ImmGetContext(window);
            ImmReleaseContext(window, context);
            !context.is_null()
        };
        let before = has_one();
        input_method(window, false);
        let off = has_one();
        input_method(window, true);
        let on = has_one();
        unsafe { DestroyWindow(window) };
        assert_eq!((before, off, on), (true, false, true));
    }
}
