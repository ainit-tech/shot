//! The keys for a screenshot, on the first start right after the choice of the language
//! (decision 7). Print Screen is chosen; other keys pressed take its place, and the program says
//! at once whether another program has them. Where Windows opens its Snipping Tool with Print
//! Screen, the window says so and turns that off on a click. A quiet tip offers Alt with the key
//! left of 1, for the left hand alone, named as the user's keyboard has it. "Next" saves the keys,
//! and the presentation of how to use the program follows with them. A page in WebView2, like the
//! program's other windows.

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};

use serde_json::{Map, Value, json};
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
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, HICON, IDC_ARROW,
    LoadCursorW, RegisterClassExW, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE, WM_DPICHANGED, WM_SIZE, WNDCLASSEXW, WS_CAPTION,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPED, WS_SYSMENU,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::{QUIET, app, browser, guide, hotkey, last_error, out_of_sight, shell, tray, wide};
use crate::settings::Settings;
use crate::text::{self, tr};

const CLASS: PCWSTR = w!("ainitShot.keys");
/// The page of the window, in the program itself.
const PAGE: &str = include_str!("keys.html");
/// The colour of the window until the page is drawn.
const BACKGROUND: u32 = 0x001C_1C1C;
/// The window's size at the scale of 100%.
const SIZE: (i32, i32) = (720, 600);

thread_local! {
    static WINDOW: RefCell<Option<Chooser>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

struct Chooser {
    window: HWND,
    controller: Option<ICoreWebView2Controller>,
    icons: Vec<HICON>,
}

/// Asks for the keys of a screenshot, and then shows how to use the program. The hotkey is held
/// back meanwhile, so that pressing it chooses it instead of freezing the screen. Without WebView2
/// nothing is asked, and the keys of the settings stay.
pub fn open() {
    if is_open() {
        return;
    }
    let window = match create_window() {
        Ok(window) => window,
        Err(error) => {
            eprintln!("The choice of the keys didn't open: {error}");
            return guide::open();
        }
    };
    let icons = tray::give_icon(window);
    WINDOW.with(|cell| *cell.borrow_mut() = Some(Chooser { window, controller: None, icons }));
    app::hold_hotkey(true);
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
        None => finish(None),
    }));
}

/// Whether the window is open.
pub fn is_open() -> bool {
    window().is_some()
}

/// The keys' names as the user's keyboard has them, for the pages that show keys: the clip's
/// words for them in the language in force, PrtSc in English, and the key left of 1 as the
/// user's layout labels it, Ё on a Russian keyboard.
pub fn names() -> Value {
    let mut names = Map::new();
    names.insert("PrintScreen".into(), "PrtSc".into());
    if let Value::Object(clip) = text::page("clip") {
        for (key, name) in clip {
            if let Some(key) = key.strip_prefix("keys.") {
                names.insert(key.into(), name);
            }
        }
    }
    if let Some(label) = hotkey::left_of_one(text::current()) {
        names.insert("~".into(), label.into());
    }
    Value::Object(names)
}

/// The window has done its work: the keys chosen go into the settings, and the presentation
/// follows. Keys that can't be saved, such as keys another program has taken since, keep the
/// window open with the reason.
fn finish(chosen: Option<String>) {
    if let Some(hotkey) = chosen.filter(|hotkey| *hotkey != app::settings().hotkey) {
        let settings = Settings { hotkey, ..app::settings().as_ref().clone() };
        if let Err(problem) = app::save_settings(settings) {
            return post(&json!({ "type": "problem", "text": problem }));
        }
    }
    close();
    app::retake_hotkey();
    guide::open();
}

pub fn close() {
    let Some(chooser) = WINDOW.with(|cell| cell.borrow_mut().take()) else { return };
    if let Some(controller) = &chooser.controller {
        let _ = unsafe { controller.Close() };
    }
    unsafe { DestroyWindow(chooser.window) };
    for icon in chooser.icons {
        unsafe { DestroyIcon(icon) };
    }
}

fn window() -> Option<HWND> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref().map(|chooser| chooser.window))
}

fn controller() -> Option<ICoreWebView2Controller> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref()?.controller.clone())
}

fn create_controller(environment: &ICoreWebView2Environment, window: HWND) {
    if self::window() != Some(window) {
        return;
    }
    let created = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |result, controller| {
        match result.and_then(|()| controller.ok_or_else(|| E_POINTER.into())) {
            Ok(controller) => {
                if attach(&controller, window).is_err() {
                    finish(None);
                }
            }
            Err(_) => finish(None),
        }
        Ok(())
    }));
    if unsafe { environment.CreateCoreWebView2Controller(ComWindow(window), &created) }.is_err() {
        finish(None);
    }
}

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
        if let Some(chooser) = cell.borrow_mut().as_mut() {
            chooser.controller = Some(controller.clone());
        }
    });
    Ok(())
}

/// What the page would ask for, in the tests that go through the first start.
#[cfg(test)]
pub fn page_says(message: &str) {
    heard(message);
}

/// What the page asks for.
fn heard(message: &str) {
    let Ok(message) = serde_json::from_str::<Value>(message) else { return };
    let hotkey = message["hotkey"].as_str().unwrap_or_default();
    match message["type"].as_str().unwrap_or_default() {
        "ready" => post(&first_look()),
        "try" => post(&tried(hotkey)),
        "next" => finish(Some(hotkey.to_string())),
        "close" => finish(None),
        "snipping" => {
            let stopped = shell::stop_print_screen_snipping();
            if let Err(problem) = &stopped {
                eprintln!("The Snipping Tool keeps Print Screen: {problem}");
            }
            post(&json!({ "type": "snipped", "done": stopped.is_ok() }));
        }
        "windowsSettings" => {
            if let Err(problem) = shell::open(shell::KEYBOARD_SETTINGS) {
                app::notify(tr("Windows Settings didn't open"), &problem);
            }
        }
        _ => {}
    }
}

/// What the page needs: its texts in the language in force, the standard keys, which come
/// chosen, the keys' names on the user's keyboard and the label for the tip.
fn first_look() -> Value {
    let language = text::current();
    json!({
        "type": "init",
        "language": language.code(),
        "rightToLeft": language.right_to_left(),
        "texts": text::page("keys"),
        "hotkey": Settings::default().hotkey,
        "names": names(),
        "tip": hotkey::left_of_one(language),
    })
}

/// Whether keys are free, and whether they are Print Screen alone while Windows opens its
/// Snipping Tool with it: so where the user's settings say so, and where they say nothing and
/// Print Screen is taken, as Windows does by default since 2023.
fn tried(text: &str) -> Value {
    let parsed = hotkey::parse(text).ok();
    let free = parsed.is_some_and(|parsed| hotkey::free(app::main_window(), parsed));
    let alone = parsed.is_some() && parsed == hotkey::parse("PrintScreen").ok();
    let snipping = alone && shell::print_screen_snips().unwrap_or(!free);
    json!({ "type": "tried", "hotkey": text, "free": free, "snipping": snipping })
}

fn post(message: &Value) {
    let Some(controller) = controller() else { return };
    let message = wide(&message.to_string());
    if let Ok(webview) = unsafe { controller.CoreWebView2() } {
        let _ = unsafe { webview.PostWebMessageAsJson(ComText(message.as_ptr())) };
    }
}

fn create_window() -> std::io::Result<HWND> {
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
        let (place, quiet) = match QUIET {
            true => (out_of_sight(place()), WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE),
            false => (place(), 0),
        };
        let window = CreateWindowExW(
            quiet,
            CLASS,
            w!("ainitShot"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
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

/// In the middle of the monitor under the mouse, at its own size for the monitor's scale.
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
        WM_CLOSE => finish(None),
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
    use crate::text::Language;

    /// The page's texts are those of the catalogs: every text the page has in English is in the
    /// Russian catalog, which every other catalog follows, and the catalog has none that the page
    /// no longer has.
    #[test]
    fn the_catalogs_have_the_pages_texts() {
        let start = PAGE.find("const TEXT = {").unwrap();
        let end = start + PAGE[start..].find("\n};").unwrap();
        let mut page: Vec<&str> =
            PAGE[start..end].lines().skip(1).filter_map(|line| Some(line.strip_prefix("  ")?.split_once(':')?.0)).collect();
        let catalog: Value = serde_json::from_str(include_str!("../locales/ru.json")).unwrap();
        let mut russian: Vec<&str> = catalog["keys"].as_object().unwrap().keys().map(String::as_str).collect();
        page.sort_unstable();
        russian.sort_unstable();
        assert_eq!(page, russian);
    }

    fn evaluate(script: &str) -> String {
        evaluate_on(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), script)
    }

    /// Opens the choice of the keys out of sight, in Russian, English, Arabic and Japanese: Print Screen
    /// comes chosen; keys pressed take its place, and a letter alone is refused; the tip offers Alt
    /// with the key left of 1 and takes it on a click; Windows' own screen capture with Print
    /// Screen is explained. The tip names the key as the user's layout labels it: the Arabic and
    /// Japanese pictures show it as an Arabic keyboard does, ذ, and as a Japanese one, where the
    /// key switches the input method and no tip is offered. With `SCREENSHOT_PREVIEW=<folder>` it
    /// saves pictures: `LOCALAPPDATA=<folder> cargo test -p ainitshot -- --ignored --exact
    /// win::keys::tests::the_choice_of_keys`.
    #[test]
    #[ignore]
    fn the_choice_of_keys() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let preview = std::env::var_os("SCREENSHOT_PREVIEW").map(PathBuf::from);
        let mut seen = Vec::new();
        let japanese = Language::from_code("ja").unwrap();
        for (language, label) in [(Language::RU, None), (Language::EN, None), (Language::AR, Some("'ذ'")), (japanese, Some("null"))] {
            text::set(language);
            open();
            let ready = pump_until(30, || controller().is_some() && evaluate("document.body.className") == "\"\"");
            assert!(ready, "the page of {} didn't get its texts", language.code());
            pump_until(1, || false);
            let code = language.code();
            let save = |name: &str| {
                pump_until(1, || false);
                if let Some(folder) = &preview {
                    let path = folder.join(format!("keys-{code}{name}.png"));
                    save_page_of(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), &path.to_string_lossy());
                }
            };
            if let Some(label) = label {
                evaluate(&format!("state.tip = {label}; if (state.tip) state.names['~'] = state.tip; render()"));
            }
            // What the program would say of Print Screen on a computer where it is free.
            evaluate("heard({ type: 'tried', hotkey: 'PrintScreen', free: true, snipping: false })");
            seen.push(evaluate("document.documentElement.lang + ' ' + state.hotkey + ' ' + $('#field').textContent + ' ' + $('#status').className"));
            save("");
            // Windows opens its own screen capture with Print Screen.
            evaluate("heard({ type: 'tried', hotkey: 'PrintScreen', free: false, snipping: true })");
            seen.push(evaluate("!$('#note').hidden"));
            save("-snipping");
            // A letter alone would get in the way of typing; Ctrl+Shift+S takes the place.
            evaluate("window.told = []; chrome.webview.postMessage = m => told.push(JSON.parse(m))");
            evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyQ', key: 'q', keyCode: 81 }))");
            seen.push(evaluate("$('#problem').textContent !== '' && state.hotkey === 'PrintScreen'"));
            evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyS', key: 's', keyCode: 83, ctrlKey: true, shiftKey: true }))");
            seen.push(evaluate("state.hotkey + ' ' + JSON.stringify(told.map(m => m.type + ':' + m.hotkey))"));
            // The tip takes Alt with the key left of 1, where there is a tip.
            if label != Some("null") {
                evaluate("told.length = 0; $('#tip .link').click(); heard({ type: 'tried', hotkey: 'Alt+~', free: true, snipping: false })");
                seen.push(evaluate("state.hotkey + ' ' + $('#tip').hidden + ' ' + JSON.stringify(told.map(m => m.type + ':' + m.hotkey))"));
                save("-tip");
            }
            evaluate("told.length = 0; document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }))");
            seen.push(evaluate("JSON.stringify(told.map(m => m.type + ':' + m.hotkey))"));
            close();
        }
        browser::close();
        let printscreen = |code: &str| format!("\"{code} PrintScreen PrtSc status free\"");
        assert_eq!(
            seen,
            [
                printscreen("ru"),
                "true".into(),
                "true".into(),
                r#""Ctrl+Shift+S [\"try:Ctrl+Shift+S\"]""#.into(),
                r#""Alt+~ true [\"try:Alt+~\"]""#.into(),
                r#""[\"next:Alt+~\"]""#.into(),
                printscreen("en"),
                "true".into(),
                "true".into(),
                r#""Ctrl+Shift+S [\"try:Ctrl+Shift+S\"]""#.into(),
                r#""Alt+~ true [\"try:Alt+~\"]""#.into(),
                r#""[\"next:Alt+~\"]""#.into(),
                printscreen("ar"),
                "true".into(),
                "true".into(),
                r#""Ctrl+Shift+S [\"try:Ctrl+Shift+S\"]""#.into(),
                r#""Alt+~ true [\"try:Alt+~\"]""#.into(),
                r#""[\"next:Alt+~\"]""#.into(),
                printscreen("ja"),
                "true".into(),
                "true".into(),
                r#""Ctrl+Shift+S [\"try:Ctrl+Shift+S\"]""#.into(),
                r#""[\"next:Ctrl+Shift+S\"]""#.into(),
            ]
        );
    }
}
