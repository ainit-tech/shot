//! The answer card (decision 8): a small dark window next to the selection with the answer of
//! AI Mode alone, while the window it comes from stays hidden. Its text can be selected and
//! copied, and Ctrl+C with nothing selected copies all of it; Esc or a click elsewhere closes
//! the card. "Показать разговор" under the text shows the hidden window with the conversation.

use std::cell::RefCell;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DT_CALCRECT, DT_EDITCONTROL, DT_NOPREFIX, DT_WORDBREAK, DrawTextW,
    EndPaint, GetDC, GetMonitorInfoW, HBRUSH, HDC, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromRect, PAINTSTRUCT, ReleaseDC, ScreenToClient, SelectObject, SetBkColor, SetTextColor,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{EM_GETSEL, EM_SETMARGINS, EM_SETSEL, SetWindowTheme, ShowScrollBar, WM_MOUSELEAVE};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_ESCAPE,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EC_LEFTMARGIN, EC_RIGHTMARGIN, ES_MULTILINE, ES_READONLY,
    GetClientRect, GetCursorPos, GetParent, HTCLIENT, HWND_TOPMOST, HideCaret, IDC_ARROW, IDC_HAND, LoadCursorW,
    MoveWindow, PostMessageW,
    RegisterClassExW, SB_VERT, SM_CXVSCROLL, SWP_NOACTIVATE, SWP_SHOWWINDOW, SendMessageW, SetCursor,
    SetForegroundWindow, SetWindowPos, SetWindowTextW, WA_INACTIVE, WM_ACTIVATE, WM_CHAR, WM_CLOSE, WM_COPY,
    WM_CTLCOLORSTATIC, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SETFOCUS, WM_SETFONT,
    WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE, WS_VSCROLL,
};
use windows_sys::core::PCWSTR;
use windows_sys::w;

use super::gdi::{self, Font};
use super::{browser, wide};
use crate::card as layout;
use crate::color::Rgba;
use crate::geometry::Rect;
use crate::text::tr;

const CLASS: PCWSTR = w!("ainitShot.card");
/// The colours of the toolbar: the background, a faint edge, the text, and the link.
const BACKGROUND: u32 = 0x0020_2020;
const EDGE: u32 = 0x0040_4040;
const INK: Rgba = [236, 236, 236, 255];
const DIM: Rgba = [154, 160, 166, 255];
/// At the scale of 100%: the text's size, the link's size, the padding around the text and the
/// height of the line with the link.
const TEXT: i32 = 15;
const LINK_TEXT: i32 = 12;
const PADDING: i32 = 14;
const LINK_LINE: i32 = 26;
/// What the card says while the answer is on its way, and the label of its link.
fn waiting() -> &'static str {
    tr("Google is thinking…")
}

fn link_label() -> &'static str {
    tr("Show the conversation")
}

thread_local! {
    static CARD: RefCell<Option<Card>> = const { RefCell::new(None) };
    /// The card's background. It is made once and never deleted: the window class, registered
    /// with the first card, keeps it for every card after.
    static BACKGROUND_BRUSH: HBRUSH = unsafe { CreateSolidBrush(BACKGROUND) };
}

struct Card {
    window: HWND,
    text_box: HWND,
    /// The selection the card belongs to and the screen around it, on the desktop.
    selection: Rect,
    screen: Rect,
    dpi: u32,
    font: Font,
    link_font: Font,
    text: String,
    waiting: bool,
    link_hovered: bool,
}

fn background() -> HBRUSH {
    BACKGROUND_BRUSH.with(|brush| *brush)
}

/// Opens the card next to the selection, given on the desktop, while the answer is awaited.
pub fn open(selection: Rect) {
    close();
    let Some(card) = create(selection) else { return };
    let window = card.window;
    CARD.with(|cell| *cell.borrow_mut() = Some(card));
    set_text(waiting(), true);
    unsafe {
        SetForegroundWindow(window);
        if let Some(text_box) = CARD.with(|cell| cell.borrow().as_ref().map(|card| card.text_box)) {
            SetFocus(text_box);
        }
    }
}

/// Shows the answer as far as it has come. Nothing happens if the card has been closed.
pub fn answer(text: &str) {
    set_text(text, false);
}

/// The card's text, or nothing if it is closed.
#[cfg(test)]
pub fn text() -> Option<String> {
    CARD.with(|cell| cell.borrow().as_ref().map(|card| card.text.clone()))
}

/// Closes the card.
pub fn close() {
    let Some(card) = CARD.with(|cell| cell.borrow_mut().take()) else { return };
    unsafe { DestroyWindow(card.window) };
}

fn create(selection: Rect) -> Option<Card> {
    let area = RECT { left: selection.left, top: selection.top, right: selection.right, bottom: selection.bottom };
    let monitor = unsafe { MonitorFromRect(&area, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let (mut dpi, mut unused) = (96, 96);
    unsafe {
        GetMonitorInfoW(monitor, &mut info);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
    }
    let work = info.rcWork;
    let screen = Rect::new(work.left, work.top, work.right, work.bottom);
    let scaled = |length: i32| (length as f32 * dpi as f32 / 96.0).round() as i32;
    let font = Font::new(scaled(TEXT), false).ok()?;
    let link_font = Font::new(scaled(LINK_TEXT), false).ok()?;
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: background(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        // The class stays registered after the first card.
        RegisterClassExW(&class);
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS,
            w!("ainitShot"),
            WS_POPUP | WS_CLIPCHILDREN,
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
            return None;
        }
        // Rounded corners and a faint edge, as Windows 11 draws its own flyouts.
        let round = DWMWCP_ROUND;
        DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&round as *const i32).cast(), 4);
        DwmSetWindowAttribute(window, DWMWA_BORDER_COLOR as u32, (&EDGE as *const u32).cast(), 4);
        let text_box = CreateWindowExW(
            0,
            w!("EDIT"),
            null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | (ES_MULTILINE | ES_READONLY) as u32,
            0,
            0,
            0,
            0,
            window,
            null_mut(),
            instance,
            null(),
        );
        if text_box.is_null() {
            DestroyWindow(window);
            return None;
        }
        SendMessageW(text_box, WM_SETFONT, font.handle() as usize, 0);
        SendMessageW(text_box, EM_SETMARGINS, (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize, 0);
        // A dark scroll bar to go with the dark card.
        SetWindowTheme(text_box, w!("DarkMode_Explorer"), null());
        SetWindowSubclass(text_box, Some(text_box_proc), 1, 0);
        Some(Card {
            window,
            text_box,
            selection,
            screen,
            dpi,
            font,
            link_font,
            text: String::new(),
            waiting: true,
            link_hovered: false,
        })
    }
}

/// Puts the text into the card and fits the card to it.
fn set_text(text: &str, waiting: bool) {
    let text_box = CARD.with(|cell| {
        let mut cell = cell.borrow_mut();
        let card = cell.as_mut()?;
        card.text = text.to_string();
        card.waiting = waiting;
        Some(card.text_box)
    });
    let Some(text_box) = text_box else { return };
    // The text box takes Windows' line ends.
    let shown = text.replace("\r\n", "\n").replace('\n', "\r\n");
    unsafe { SetWindowTextW(text_box, wide(&shown).as_ptr()) };
    fit();
}

/// Sizes the card to its text and puts it next to the selection.
fn fit() {
    CARD.with(|cell| {
        let cell = cell.borrow();
        let Some(card) = cell.as_ref() else { return };
        let scale = card.dpi as f32 / 96.0;
        let scaled = |length: i32| (length as f32 * scale).round() as i32;
        let (padding, link_line) = (scaled(PADDING), scaled(LINK_LINE));
        let scroll_bar = unsafe { GetSystemMetricsForDpi(SM_CXVSCROLL, card.dpi) };
        let dc = unsafe { GetDC(card.window) };
        let longest =
            card.text.lines().map(|line| gdi::text_size(dc, &card.font, &line.encode_utf16().collect::<Vec<_>>()).0).max();
        let longest = longest.unwrap_or(0);
        let frame = 2 * padding;
        // A little more height than the lines need keeps the text box from scrolling by a pixel.
        let text_height = |width: i32| wrapped_height(dc, &card.font, &card.text, width) + scaled(4);
        let mut width = layout::width(longest, frame, scale, card.screen);
        let (mut height, mut scrolls) = layout::height(text_height(width - frame), frame + link_line, scale, card.screen);
        if scrolls {
            // The scroll bar takes some of the width, and the text wraps narrower.
            width = layout::width(longest, frame + scroll_bar, scale, card.screen);
            (height, scrolls) = layout::height(text_height(width - frame - scroll_bar), frame + link_line, scale, card.screen);
        }
        unsafe { ReleaseDC(card.window, dc) };
        let place = layout::place(card.selection, (width, height), scale, card.screen);
        unsafe {
            SetWindowPos(card.window, HWND_TOPMOST, place.left, place.top, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            MoveWindow(card.text_box, padding, padding, width - frame, height - frame - link_line + padding / 2, 1);
            ShowScrollBar(card.text_box, SB_VERT, scrolls as i32);
            InvalidateRect(card.window, null(), 1);
        }
    });
}

/// The height of the text wrapped at the given width.
fn wrapped_height(dc: HDC, font: &Font, text: &str, width: i32) -> i32 {
    let text: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT { left: 0, top: 0, right: width.max(1), bottom: 0 };
    unsafe {
        let previous = SelectObject(dc, font.handle());
        DrawTextW(dc, text.as_ptr(), text.len() as i32, &mut rect, DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX);
        SelectObject(dc, previous);
    }
    rect.bottom - rect.top
}

/// Where the link "Show the conversation" is in the card, and its label.
fn link(card: &Card, dc: HDC) -> (RECT, Vec<u16>, (i32, i32)) {
    let scale = card.dpi as f32 / 96.0;
    let scaled = |length: i32| (length as f32 * scale).round() as i32;
    let label: Vec<u16> = link_label().encode_utf16().collect();
    let size = gdi::text_size(dc, &card.link_font, &label);
    let mut client = RECT::default();
    unsafe { GetClientRect(card.window, &mut client) };
    let (right, bottom) = (client.right - scaled(PADDING), client.bottom - scaled(PADDING) / 2);
    let place = RECT { left: right - size.0, top: bottom - scaled(LINK_LINE) + scaled(PADDING) / 2, right, bottom };
    (place, label, size)
}

fn paint(window: HWND) {
    let mut painting = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(window, &mut painting) };
    CARD.with(|cell| {
        if let Ok(cell) = cell.try_borrow()
            && let Some(card) = cell.as_ref()
        {
            let (place, label, size) = link(card, dc);
            let y = place.top + (place.bottom - place.top - size.1) / 2;
            gdi::draw_text(dc, &card.link_font, &label, place.left, y, if card.link_hovered { INK } else { DIM });
        }
    });
    unsafe { EndPaint(window, &painting) };
}

/// Whether the point of the card is on the link.
fn on_link(window: HWND, point: POINT) -> bool {
    CARD.with(|cell| {
        let Ok(cell) = cell.try_borrow() else { return false };
        let Some(card) = cell.as_ref() else { return false };
        let dc = unsafe { GetDC(window) };
        let (place, _, _) = link(card, dc);
        unsafe { ReleaseDC(window, dc) };
        point.x >= place.left && point.x < place.right && point.y >= place.top && point.y < place.bottom
    })
}

fn hover(window: HWND, over: bool) {
    let changed = CARD.with(|cell| {
        let mut cell = cell.borrow_mut();
        let card = cell.as_mut()?;
        let changed = card.link_hovered != over;
        card.link_hovered = over;
        Some(changed)
    });
    if changed == Some(true) {
        unsafe { InvalidateRect(window, null(), 1) };
    }
    if over {
        let mut track =
            TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: window, dwHoverTime: 0 };
        unsafe { TrackMouseEvent(&mut track) };
    }
}

fn point_of(lparam: LPARAM) -> POINT {
    POINT { x: (lparam & 0xFFFF) as i16 as i32, y: ((lparam >> 16) & 0xFFFF) as i16 as i32 }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT => paint(window),
        WM_CTLCOLORSTATIC => {
            // The read-only text box asks for its colours: light on the card's own background,
            // dimmer while the answer is awaited.
            let waiting = CARD.with(|cell| cell.try_borrow().ok().and_then(|cell| cell.as_ref().map(|card| card.waiting)));
            let [red, green, blue, _] = if waiting == Some(true) { DIM } else { INK };
            let dc = wparam as HDC;
            unsafe {
                SetTextColor(dc, red as u32 | (green as u32) << 8 | (blue as u32) << 16);
                SetBkColor(dc, BACKGROUND);
            }
            return background() as LRESULT;
        }
        WM_MOUSEMOVE => hover(window, on_link(window, point_of(lparam))),
        WM_MOUSELEAVE => hover(window, false),
        WM_LBUTTONUP => {
            if on_link(window, point_of(lparam)) {
                browser::show();
            }
        }
        WM_SETCURSOR if (lparam & 0xFFFF) as u32 == HTCLIENT && wparam as HWND == window => {
            let mut point = POINT::default();
            unsafe {
                GetCursorPos(&mut point);
                ScreenToClient(window, &mut point);
                SetCursor(LoadCursorW(null_mut(), if on_link(window, point) { IDC_HAND } else { IDC_ARROW }));
            }
            return 1;
        }
        // A click elsewhere closes the card, after the click has been handled.
        WM_ACTIVATE if (wparam & 0xFFFF) as u32 == WA_INACTIVE => unsafe {
            PostMessageW(window, WM_CLOSE, 0, 0);
        },
        WM_CLOSE => close(),
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

/// The text box's own keys: Esc closes the card, Ctrl+A selects all of the text, and Ctrl+C
/// with nothing selected copies all of it.
unsafe extern "system" fn text_box_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
    match message {
        WM_KEYDOWN if wparam as u16 == VK_ESCAPE => {
            // Closed after this message, not while the text box is still handling it.
            unsafe { PostMessageW(GetParent(window), WM_CLOSE, 0, 0) };
            return 0;
        }
        WM_KEYDOWN if ctrl && wparam == b'A' as usize => {
            unsafe { SendMessageW(window, EM_SETSEL, 0, -1) };
            return 0;
        }
        WM_KEYDOWN if ctrl && wparam == b'C' as usize => {
            let (mut start, mut end) = (0u32, 0u32);
            unsafe { SendMessageW(window, EM_GETSEL, (&mut start as *mut u32) as usize, (&mut end as *mut u32) as isize) };
            if start == end {
                unsafe {
                    SendMessageW(window, EM_SETSEL, 0, -1);
                    SendMessageW(window, WM_COPY, 0, 0);
                    SendMessageW(window, EM_SETSEL, start as usize, end as isize);
                }
                return 0;
            }
        }
        // Esc and Ctrl+A also come as characters; the text box would beep at them.
        WM_CHAR if wparam == 0x1B || wparam == 0x01 => return 0,
        WM_SETFOCUS => {
            // The text can't be changed, so no blinking caret in it.
            let handled = unsafe { DefSubclassProc(window, message, wparam, lparam) };
            unsafe { HideCaret(window) };
            return handled;
        }
        _ => {}
    }
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Graphics::Gdi::GetPixel;
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};

    use super::*;

    fn pump(time: Duration) {
        let start = Instant::now();
        while start.elapsed() < time {
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
                unsafe { DispatchMessageW(&message) };
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Opens the card twice and checks that the second one is as dark as the first. It shows
    /// the cards on the screen, so it runs only when asked:
    /// `cargo test -p ainitshot second_card -- --ignored`.
    #[test]
    #[ignore]
    fn a_second_card_is_as_dark_as_the_first() {
        for _ in 0..2 {
            open(Rect::new(200, 200, 500, 300));
            answer("Проверка");
            pump(Duration::from_millis(300));
            let window = CARD.with(|cell| cell.borrow().as_ref().map(|card| card.window)).expect("no card");
            let mut client = RECT::default();
            unsafe { GetClientRect(window, &mut client) };
            let dc = unsafe { GetDC(window) };
            // In the padding left of the text, away from the rounded corners.
            let color = unsafe { GetPixel(dc, 4, client.bottom / 2) };
            unsafe { ReleaseDC(window, dc) };
            close();
            assert_eq!(color, BACKGROUND, "the card's background is {color:06x}");
        }
    }
}
