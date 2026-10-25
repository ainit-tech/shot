//! «Как пользоваться»: a short presentation of the main things, from the menu of the icon in
//! the notification area, and by itself on the program's first start. A page in WebView2, made
//! with the environment that the Google window keeps, with the keys from the settings in force:
//! a few slides, each with a moving picture of the frozen screen and a few lines. It is made when
//! opened and let go when closed.

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};

use serde_json::{Value, json};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, ICoreWebView2Controller, ICoreWebView2Controller2,
    ICoreWebView2Environment, ICoreWebView2Settings3,
};
use webview2_com::{CreateCoreWebView2ControllerCompletedHandler, WebMessageReceivedEventHandler, take_pwstr};
use windows::Win32::Foundation::{E_POINTER, HWND as ComWindow, RECT as ComRect};
use windows_core::{Interface, PCWSTR as ComText, PWSTR as ComString};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, HICON, IDC_ARROW, IsIconic,
    LoadCursorW, RegisterClassExW, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos,
    SetWindowTextW, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE, WM_DPICHANGED, WM_MOVE, WM_SIZE, WNDCLASSEXW, WS_CAPTION,
    WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::{app, browser, invite, last_error, preferences, tray, wide};
use crate::annotation::Tool;
use crate::icons::{self, Part};
use crate::text::{self, tr};

const CLASS: PCWSTR = w!("ainitShot.guide");
/// The page of the window, in the program itself.
const PAGE: &str = include_str!("guide.html");
/// The colour of the window until the page is drawn.
const BACKGROUND: u32 = 0x001C_1C1C;
/// The window's size at the scale of 100%: a presentation keeps its size.
const SIZE: (i32, i32) = (900, 640);

thread_local! {
    static WINDOW: RefCell<Option<Guide>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
    /// Whether the invitation to sign in to Google waits for the presentation to close.
    static INVITE: Cell<bool> = const { Cell::new(false) };
}

struct Guide {
    window: HWND,
    /// Comes a moment after the window, once WebView2 has made it.
    controller: Option<ICoreWebView2Controller>,
    /// The program's icon on the window's title bar and on the taskbar.
    icons: Vec<HICON>,
}

/// Opens the presentation from its first slide, or brings it to the front if it is open.
pub fn open() {
    if let Some(window) = window() {
        bring_up(window);
        return;
    }
    let window = match create_window() {
        Ok(window) => window,
        Err(error) => {
            app::notify(not_open(), &error.to_string());
            return;
        }
    };
    let icons = tray::give_icon(window);
    WINDOW.with(|cell| *cell.borrow_mut() = Some(Guide { window, controller: None, icons }));
    bring_up(window);
    browser::with_environment(Box::new(move |environment| match environment {
        Some(environment) => create_controller(environment, window),
        None => {
            close();
            app::notify(not_open(), tr("WebView2 didn't start."));
        }
    }));
}

/// The settings in force have changed: the slides show the new keys.
pub fn refresh() {
    if let Some(window) = window() {
        unsafe { SetWindowTextW(window, wide(&title()).as_ptr()) };
    }
    post(&speaking(json!({ "type": "settings", "settings": in_force() })));
}

/// A message to the page with what it needs to speak the language in force: its code, the
/// slides' texts in it, whether it is written from right to left, and the tools with their keys
/// and their names in it.
fn speaking(mut message: Value) -> Value {
    let language = text::current();
    let tools: Vec<Value> =
        Tool::ALL.iter().map(|tool| json!({ "id": format!("{tool:?}"), "key": tool.key().to_string(), "name": tool.name() })).collect();
    if let Value::Object(fields) = &mut message {
        fields.insert("language".into(), language.code().into());
        fields.insert("texts".into(), text::page("guide"));
        fields.insert("rightToLeft".into(), language.right_to_left().into());
        fields.insert("tools".into(), tools.into());
    }
    message
}

fn not_open() -> &'static str {
    tr("The presentation didn't open")
}

/// The window's title in the language in force.
fn title() -> String {
    format!("{} — ainitShot", tr("How to use"))
}

/// Whether someone is signed in to Google in the program's window: the last slide offers to sign
/// in while nobody is.
pub fn account(signed_in: bool) {
    post(&json!({ "type": "account", "signedIn": signed_in }));
}

/// Whether the presentation is open.
pub fn is_open() -> bool {
    window().is_some()
}

/// The invitation to sign in to Google comes once the user has closed the presentation, so that
/// the two don't show at once; the last slide offers to sign in meanwhile.
pub fn invite_when_closed() {
    INVITE.set(true);
}

/// The user has closed the presentation: an invitation that waited for it comes, if nobody has
/// signed in meanwhile.
fn closed_by_user() {
    close();
    invite_if_waiting();
}

/// The invitation to sign in that waited for the windows of the start comes, if nobody has signed
/// in meanwhile.
pub fn invite_if_waiting() {
    if INVITE.take() && app::signed_in() != Some(true) {
        invite::open();
    }
}

/// Closes the window and lets its page go.
pub fn close() {
    let Some(guide) = WINDOW.with(|cell| cell.borrow_mut().take()) else { return };
    if let Some(controller) = &guide.controller {
        let _ = unsafe { controller.Close() };
    }
    unsafe { DestroyWindow(guide.window) };
    for icon in guide.icons {
        unsafe { DestroyIcon(icon) };
    }
}

fn window() -> Option<HWND> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref().map(|guide| guide.window))
}

fn controller() -> Option<ICoreWebView2Controller> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref()?.controller.clone())
}

fn bring_up(window: HWND) {
    unsafe {
        ShowWindow(window, if IsIconic(window) != 0 { SW_RESTORE } else { SW_SHOW });
        SetForegroundWindow(window);
    }
}

fn create_controller(environment: &ICoreWebView2Environment, window: HWND) {
    // The window may have been closed while WebView2 was starting.
    if self::window() != Some(window) {
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

/// Puts the page into the window: dark until it is drawn, without the browser's own keys, zoom
/// and status bar, and with the page's messages heard.
fn attach(controller: &ICoreWebView2Controller, window: HWND) -> windows_core::Result<()> {
    if self::window() != Some(window) {
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
        if let Some(guide) = cell.borrow_mut().as_mut() {
            guide.controller = Some(controller.clone());
        }
    });
    Ok(())
}

/// What the page asks for.
fn heard(message: &str) {
    let Ok(message) = serde_json::from_str::<Value>(message) else { return };
    match message["type"].as_str().unwrap_or_default() {
        "ready" => post(&first_look()),
        "settings" => preferences::open(),
        "account" => {
            if let Err(problem) = browser::open(browser::ACCOUNT) {
                app::notify(browser::window_not_open(), &problem);
            }
        }
        "close" => closed_by_user(),
        _ => {}
    }
}

/// What the slides need: the settings in force, whether someone is signed in to Google, the
/// tools with their keys, and the icons of the toolbar, to draw it as the program does.
fn first_look() -> Value {
    speaking(json!({
        "type": "init",
        "settings": in_force(),
        "signedIn": app::signed_in(),
        "icons": {
            "Pencil": parts(icons::PENCIL),
            "Brush": parts(icons::BRUSH),
            "Line": parts(icons::LINE_TOOL),
            "Arrow": parts(icons::ARROW),
            "Rectangle": parts(icons::RECTANGLE),
            "FilledRectangle": parts(icons::FILLED_RECTANGLE),
            "Text": parts(icons::TEXT),
            "MagicPencil": parts(icons::MAGIC_PENCIL),
            "undo": parts(icons::UNDO),
            "copy": parts(icons::COPY),
            "copyText": parts(icons::TEXT_COPY),
            "save": parts(icons::SAVE),
            "close": parts(icons::CLOSE),
            "images": parts(icons::IMAGES),
            "question": parts(icons::QUESTION),
        },
    }))
}

fn in_force() -> Value {
    serde_json::to_value(app::settings().as_ref()).unwrap_or_default()
}

/// An icon of the toolbar as the page draws it: lines, fills and dots on the grid of 20 by 20.
fn parts(icon: &[Part]) -> Value {
    icon.iter()
        .map(|part| match part {
            Part::Line(d) => json!({ "line": d }),
            Part::Fill(d) => json!({ "fill": d }),
            Part::Dot(x, y, r) => json!({ "dot": [x, y, r] }),
        })
        .collect()
}

/// Tells the page something; nothing happens if the page isn't there.
fn post(message: &Value) {
    let Some(controller) = controller() else { return };
    let message = wide(&message.to_string());
    if let Ok(webview) = unsafe { controller.CoreWebView2() } {
        let _ = unsafe { webview.PostWebMessageAsJson(ComText(message.as_ptr())) };
    }
}

fn failed(error: &windows_core::Error) {
    close();
    app::notify(not_open(), &error.message());
}

fn create_window() -> std::io::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(null());
        // The class stays registered after the first window.
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
        let place = place();
        let window = CreateWindowExW(
            0,
            CLASS,
            wide(&title()).as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
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
        // A dark title bar, like the rest of the program.
        let dark: BOOL = 1;
        DwmSetWindowAttribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, (&dark as *const BOOL).cast(), size_of::<BOOL>() as u32);
        Ok(window)
    }
}

/// Where the window appears: in the middle of the monitor under the mouse, at its own size for
/// the monitor's scale, or smaller on a small screen.
fn place() -> RECT {
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
    let (w, h) = (scaled(SIZE.0).min(work.right - work.left), scaled(SIZE.1).min(work.bottom - work.top));
    let (left, top) = (work.left + (work.right - work.left - w) / 2, work.top + (work.bottom - work.top - h) / 2);
    RECT { left, top, right: left + w, bottom: top + h }
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
        WM_CLOSE => closed_by_user(),
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

    use super::super::browser::tests::{evaluate_on, pump_until, save_page_of};
    use super::*;

    fn evaluate(script: &str) -> String {
        evaluate_on(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), script)
    }

    /// Shows the presentation in every language the program speaks: every text of the slides has
    /// all three. With `SCREENSHOT_PREVIEW=<folder>` it saves the slide of the queries in each:
    /// `LOCALAPPDATA=<folder> cargo test -p ainitshot slides_speak -- --ignored`.
    #[test]
    #[ignore]
    fn the_slides_speak_every_language() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        open();
        let ready = pump_until(30, || controller().is_some() && evaluate("document.body.className") == "\"\"");
        let preview = std::env::var_os("SCREENSHOT_PREVIEW").map(PathBuf::from);
        let mut seen = Vec::new();
        if ready {
            for language in text::Language::all().chain([text::Language::RU]) {
                text::set(language);
                refresh();
                evaluate("show(4)");
                pump_until(2, || false);
                let missing = "Object.keys(TEXT).filter(key => state.language !== 'en' && !state.texts[key])";
                seen.push(evaluate(&format!("document.documentElement.lang + ' ' + JSON.stringify({missing})")));
                if let Some(folder) = &preview {
                    let path = folder.join(format!("guide-{}.png", language.code()));
                    save_page_of(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), &path.to_string_lossy());
                }
            }
        }
        close();
        browser::close();
        assert!(ready, "the page didn't get its settings");
        let expected: Vec<String> =
            text::Language::all().chain([text::Language::RU]).map(|l| format!("\"{} []\"", l.code())).collect();
        assert_eq!(seen, expected);
    }

    /// Opens the presentation with the default settings and goes through its slides with the
    /// keys: every slide has its title and its picture, and the slide of the queries tells how a
    /// conversation's button is pressed and held. It starts WebView2, whose Google window goes
    /// online, so it runs only when asked: `LOCALAPPDATA=<folder> cargo test -p ainitshot
    /// presentation -- --ignored`. With `SCREENSHOT_PREVIEW=<folder>` it also saves each slide.
    #[test]
    #[ignore]
    fn the_presentation_goes_through_its_slides() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        open();
        let ready = pump_until(30, || controller().is_some() && evaluate("document.body.className") == "\"\"");
        let preview = std::env::var_os("SCREENSHOT_PREVIEW").map(PathBuf::from);
        let mut titles = Vec::new();
        let mut held = String::new();
        if ready {
            let count: usize = evaluate("SLIDES.length").parse().unwrap_or(0);
            for i in 0..count {
                // Halfway through its moving picture.
                pump_until(2, || false);
                titles.push(evaluate("document.querySelector('h1').textContent"));
                assert_ne!(evaluate("document.querySelector('#scene').childElementCount"), "0", "slide {i} has a picture");
                let save = |name: String| {
                    if let Some(folder) = &preview {
                        let path = folder.join(name);
                        save_page_of(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), &path.to_string_lossy());
                    }
                };
                save(format!("guide-{}.png", i + 1));
                if evaluate("document.querySelector('h1').textContent").contains("Google") {
                    held = evaluate("document.querySelector('#points').textContent");
                    // And while the key is held.
                    pump_until(3, || false);
                    save(format!("guide-{}-held.png", i + 1));
                }
                evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }))");
            }
        }
        close();
        browser::close();
        assert!(ready, "the page didn't get its settings");
        assert!(titles.len() >= 5, "{titles:?}");
        assert!(held.contains("нажми") && held.contains("зажми"), "{held}");
    }
}
