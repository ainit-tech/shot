//! The invitation to sign in to Google (decision 8): a small card by the icon in the notification
//! area, which the program shows as it starts if nobody is signed in in its Google window.
//! Windows holds its own notifications back while it takes some window for a full-screen game, as
//! it does with NVIDIA's overlay, or while the user doesn't want to be disturbed, so the program
//! shows the card itself. The card doesn't take the keyboard; "Войти" opens Google's sign-in in
//! the program's window, and the card goes by itself after half a minute unless the mouse is on
//! it. Its page is in WebView2, like the settings window's.

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};

use serde_json::Value;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, ICoreWebView2Controller, ICoreWebView2Controller2, ICoreWebView2Environment, ICoreWebView2Settings3,
};
use webview2_com::{CreateCoreWebView2ControllerCompletedHandler, WebMessageReceivedEventHandler, take_pwstr};
use windows::Win32::Foundation::{E_POINTER, HWND as ComWindow, RECT as ComRect};
use windows_core::{Interface, PCWSTR as ComText, PWSTR as ComString};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromRect,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Shell::{ABE_LEFT, ABE_TOP, ABM_GETTASKBARPOS, APPBARDATA, SHAppBarMessage};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, HWND_TOPMOST, IDC_ARROW, LoadCursorW, MA_NOACTIVATE,
    RegisterClassExW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetWindowPos, ShowWindow, WM_CLOSE, WM_MOUSEACTIVATE, WM_SIZE,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::{app, browser, last_error, wide};
use crate::text;

const CLASS: PCWSTR = w!("ainitShot.invite");
/// The card's page, in the program itself.
const PAGE: &str = include_str!("invite.html");
/// The colours of the program's cards: the background and a faint edge.
const BACKGROUND: u32 = 0x0020_2020;
const EDGE: u32 = 0x0040_4040;
/// The card's size at the scale of 100% until its page has said its own, and the space between
/// the card and the edges of the screen.
const SIZE: (i32, i32) = (380, 150);
const GAP: i32 = 12;

thread_local! {
    static INVITE: RefCell<Option<Invite>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

struct Invite {
    window: HWND,
    /// Comes a moment after the window, once WebView2 has made it.
    controller: Option<ICoreWebView2Controller>,
}

/// Shows the invitation to sign in to Google, once its page is drawn.
pub fn open() {
    if window().is_some() {
        return;
    }
    let window = match create_window() {
        Ok(window) => window,
        Err(error) => {
            eprintln!("The invitation to sign in to Google didn't open: {error}");
            return;
        }
    };
    INVITE.with(|cell| *cell.borrow_mut() = Some(Invite { window, controller: None }));
    browser::with_environment(Box::new(move |environment| match environment {
        Some(environment) => create_controller(environment, window),
        None => close(),
    }));
}

/// Closes the card and lets its page go.
pub fn close() {
    let Some(invite) = INVITE.with(|cell| cell.borrow_mut().take()) else { return };
    if let Some(controller) = &invite.controller {
        let _ = unsafe { controller.Close() };
    }
    unsafe { DestroyWindow(invite.window) };
}

fn window() -> Option<HWND> {
    INVITE.with(|cell| cell.try_borrow().ok()?.as_ref().map(|i| i.window))
}

fn controller() -> Option<ICoreWebView2Controller> {
    INVITE.with(|cell| cell.try_borrow().ok()?.as_ref()?.controller.clone())
}

fn create_controller(environment: &ICoreWebView2Environment, window: HWND) {
    // The card may have been closed while WebView2 was starting.
    if self::window() != Some(window) {
        return;
    }
    let created = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |result, controller| {
        match result.and_then(|()| controller.ok_or_else(|| E_POINTER.into())) {
            Ok(controller) => {
                if let Err(error) = attach(&controller, window) {
                    eprintln!("The invitation to sign in to Google didn't open: {}", error.message());
                    close();
                }
            }
            Err(error) => {
                eprintln!("The invitation to sign in to Google didn't open: {}", error.message());
                close();
            }
        }
        Ok(())
    }));
    if let Err(error) = unsafe { environment.CreateCoreWebView2Controller(ComWindow(window), &created) } {
        eprintln!("The invitation to sign in to Google didn't open: {}", error.message());
        close();
    }
}

/// Puts the page into the card: dark until it is drawn, without the browser's own keys, menu,
/// zoom and status bar, and with the page's messages heard.
fn attach(controller: &ICoreWebView2Controller, window: HWND) -> windows_core::Result<()> {
    if self::window() != Some(window) {
        return unsafe { controller.Close() };
    }
    unsafe {
        controller.SetBounds(client_rect(window))?;
        // The card is hidden until its page is drawn; WebView2 hears when it is shown.
        controller.SetIsVisible(false)?;
        let dark = COREWEBVIEW2_COLOR { A: 255, R: 0x20, G: 0x20, B: 0x20 };
        controller.cast::<ICoreWebView2Controller2>()?.SetDefaultBackgroundColor(dark)?;
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetIsZoomControlEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.cast::<ICoreWebView2Settings3>()?.SetAreBrowserAcceleratorKeysEnabled(false)?;
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
        // The card speaks the language in force; `<` can't end the page's script in the texts.
        let language = text::current();
        let speech = serde_json::json!({
            "language": language.code(),
            "texts": text::page("invite"),
            "rightToLeft": language.right_to_left(),
        });
        let page = wide(&PAGE.replace("{{speech}}", &speech.to_string().replace('<', "\\u003c")));
        webview.NavigateToString(ComText(page.as_ptr()))?;
    }
    INVITE.with(|cell| {
        if let Some(invite) = cell.borrow_mut().as_mut() {
            invite.controller = Some(controller.clone());
        }
    });
    Ok(())
}

/// What the page says: its size, once it is drawn, and what the user chose.
fn heard(message: &str) {
    let Ok(message) = serde_json::from_str::<Value>(message) else { return };
    match message["type"].as_str().unwrap_or_default() {
        "size" => {
            let Some(window) = window() else { return };
            let css = |name: &str, default: i32| message[name].as_f64().map_or(default, |v| v.ceil() as i32);
            let dpi = unsafe { GetDpiForWindow(window) }.max(96) as i32;
            let (width, height) = (css("width", SIZE.0) * dpi / 96, css("height", SIZE.1) * dpi / 96);
            let place = place(width, height, dpi);
            unsafe {
                SetWindowPos(window, HWND_TOPMOST, place.left, place.top, width, height, SWP_NOACTIVATE);
                ShowWindow(window, SW_SHOWNOACTIVATE);
            }
            if let Some(controller) = controller() {
                let _ = unsafe { controller.SetIsVisible(true) };
            }
        }
        "signIn" => {
            close();
            if let Err(problem) = browser::open(browser::ACCOUNT) {
                app::notify(browser::window_not_open(), &problem);
            }
        }
        "later" => close(),
        _ => {}
    }
}

/// Where the card goes: in the corner by the notification area, at the end of the taskbar, on the
/// taskbar's monitor.
fn place(width: i32, height: i32, dpi: i32) -> RECT {
    let mut bar = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, ..Default::default() };
    let found = unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut bar) } != 0;
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(MonitorFromRect(&bar.rc, MONITOR_DEFAULTTOPRIMARY), &mut info) };
    let work = info.rcWork;
    let gap = GAP * dpi / 96;
    let (left, top) = match bar.uEdge {
        ABE_TOP if found => (work.right - width - gap, work.top + gap),
        ABE_LEFT if found => (work.left + gap, work.bottom - height - gap),
        _ => (work.right - width - gap, work.bottom - height - gap),
    };
    RECT { left, top, right: left + width, bottom: top + height }
}

fn client_rect(window: HWND) -> ComRect {
    let mut rect = RECT::default();
    unsafe { GetClientRect(window, &mut rect) };
    ComRect { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }
}

fn create_window() -> std::io::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(null());
        // The class stays registered after the first card.
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
        // Made hidden in its corner, so that it has that monitor's scale before it is shown.
        let place = place(SIZE.0, SIZE.1, 96);
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            CLASS,
            w!("ainitShot"),
            WS_POPUP,
            place.left,
            place.top,
            SIZE.0,
            SIZE.1,
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
        let round = DWMWCP_ROUND;
        DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&round as *const i32).cast(), 4);
        DwmSetWindowAttribute(window, DWMWA_BORDER_COLOR as u32, (&EDGE as *const u32).cast(), 4);
        Ok(window)
    }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_SIZE => {
            if let Some(controller) = controller() {
                let _ = unsafe { controller.SetBounds(client_rect(window)) };
            }
        }
        // A click on the card doesn't take the keyboard from the window the user works in.
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_CLOSE => close(),
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};

    use super::super::browser::tests::{evaluate_on, pump_until, save_page_of};
    use super::*;

    /// Shows the invitation, checks that it comes out by the notification area without taking the
    /// keyboard, and that "Позже" closes it. It shows the card and starts WebView2, whose Google
    /// window goes online, so it runs only when asked:
    /// `LOCALAPPDATA=<folder> cargo test -p ainitshot invitation -- --ignored`. With
    /// `SCREENSHOT_PREVIEW=<file.png>` it also saves what the card shows.
    #[test]
    #[ignore]
    fn the_invitation_comes_out_by_the_notification_area_and_goes() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        open();
        let shown = pump_until(30, || window().is_some_and(|w| unsafe { IsWindowVisible(w) } != 0));
        let mut rect = RECT::default();
        if let Some(window) = window() {
            unsafe { GetWindowRect(window, &mut rect) };
        }
        let expected = place(rect.right - rect.left, rect.bottom - rect.top, unsafe { GetDpiForWindow(window().unwrap_or(null_mut())) }.max(96) as i32);
        if let (true, Some(path)) = (shown, std::env::var_os("SCREENSHOT_PREVIEW")) {
            pump_until(1, || false);
            save_page_of(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), &path.to_string_lossy());
        }
        let seen = shown && evaluate_on(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), "document.visibilityState") == "\"visible\"";
        if shown {
            evaluate_on(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), "document.getElementById('later').click()");
            pump_until(5, || window().is_none());
        }
        let closed = window().is_none();
        close();
        browser::close();
        assert!(shown, "the card didn't come out");
        assert!(seen, "the card's page doesn't know it is shown");
        assert_eq!((rect.left, rect.top), (expected.left, expected.top), "the card isn't by the notification area");
        assert!(closed, "«Позже» didn't close the card");
    }
}
