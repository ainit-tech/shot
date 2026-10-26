//! The choice of the program's language on its first start, and on a start with a settings file
//! that has never chosen one: a window of tiles with flags, the six official languages of the UN
//! first and then every language in alphabetical order, the one of Windows marked. The window
//! speaks the language in force, which is in focus: that of Windows, or that of the installation
//! when it was another (decision 11). The language chosen goes into the settings; on the first
//! start the choice of the keys for a screenshot then opens in it, and the presentation of how to
//! use the program after that. A page in WebView2, like the program's other windows.

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
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, HICON, IDC_ARROW,
    LoadCursorW, RegisterClassExW, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE, WM_DPICHANGED, WM_SIZE, WNDCLASSEXW, WS_CAPTION,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPED, WS_SYSMENU,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::{QUIET, app, browser, guide, keys, last_error, out_of_sight, shell, tray, wide};
use crate::settings::Settings;
use crate::text::{self, Choice, Language};

const CLASS: PCWSTR = w!("ainitShot.languages");
/// The page of the window, in the program itself.
const PAGE: &str = include_str!("languages.html");
/// The colour of the window until the page is drawn.
const BACKGROUND: u32 = 0x001C_1C1C;
/// The window's size at the scale of 100%.
const SIZE: (i32, i32) = (760, 640);

thread_local! {
    static WINDOW: RefCell<Option<Picker>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

struct Picker {
    window: HWND,
    controller: Option<ICoreWebView2Controller>,
    icons: Vec<HICON>,
    /// Whether the choice of the keys and the presentation follow, as on the first start.
    first: bool,
}

/// Asks which language the program speaks; on the first start, `first`, the choice of the keys
/// and the presentation of how to use the program follow. Without WebView2 nothing is asked, and
/// the language of Windows stays.
pub fn open(first: bool) {
    let window = match create_window() {
        Ok(window) => window,
        Err(error) => {
            eprintln!("The choice of a language didn't open: {error}");
            return after(first);
        }
    };
    let icons = tray::give_icon(window);
    WINDOW.with(|cell| *cell.borrow_mut() = Some(Picker { window, controller: None, icons, first }));
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
        None => finish(),
    }));
}

/// Whether the window is open.
pub fn is_open() -> bool {
    window().is_some()
}

/// The window has done its work, chosen or closed: the choice of the keys follows on the first
/// start, and an invitation to sign in that waited for the window comes otherwise.
fn finish() {
    let first = WINDOW.with(|cell| cell.borrow().as_ref().is_some_and(|picker| picker.first));
    close();
    after(first);
}

fn after(first: bool) {
    if first {
        keys::open();
    } else {
        guide::invite_if_waiting();
    }
}

/// The window closed without a choice: the language in force stays, and goes into the settings,
/// as "auto" for that of Windows, so that it isn't asked again.
fn keep() {
    if let Err(problem) = app::save_settings(app::settings().as_ref().clone()) {
        eprintln!("The language wasn't saved: {problem}");
    }
    finish();
}

/// Puts the language chosen into the settings, which speak it from then on, standard buttons and
/// all.
fn choose(language: Language) {
    let settings = Settings { language: Choice::Own(language), ..app::settings().as_ref().clone() };
    if let Err(problem) = app::save_settings(settings) {
        eprintln!("The language wasn't saved: {problem}");
        text::set(language);
    }
    finish();
}

pub fn close() {
    let Some(picker) = WINDOW.with(|cell| cell.borrow_mut().take()) else { return };
    if let Some(controller) = &picker.controller {
        let _ = unsafe { controller.Close() };
    }
    unsafe { DestroyWindow(picker.window) };
    for icon in picker.icons {
        unsafe { DestroyIcon(icon) };
    }
}

fn window() -> Option<HWND> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref().map(|picker| picker.window))
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
                    finish();
                }
            }
            Err(_) => finish(),
        }
        Ok(())
    }));
    if unsafe { environment.CreateCoreWebView2Controller(ComWindow(window), &created) }.is_err() {
        finish();
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
        if let Some(picker) = cell.borrow_mut().as_mut() {
            picker.controller = Some(controller.clone());
        }
    });
    Ok(())
}

/// What the page asks for.
fn heard(message: &str) {
    let Ok(message) = serde_json::from_str::<Value>(message) else { return };
    match message["type"].as_str().unwrap_or_default() {
        "ready" => post(&first_look()),
        "choose" => match message["language"].as_str().and_then(Language::from_code) {
            Some(language) => choose(language),
            None => keep(),
        },
        "close" => keep(),
        _ => {}
    }
}

/// What the page needs: the languages there are, the UN's among them, the one of Windows, which
/// the page speaks, and its texts in it.
fn first_look() -> Value {
    let windows = shell::windows_language();
    let languages: Vec<Value> = Language::all().map(|l| json!({ "code": l.code(), "native": l.native() })).collect();
    json!({
        "type": "init",
        "language": text::current().code(),
        "windows": windows.code(),
        "texts": text::page("languages"),
        "rightToLeft": text::current().right_to_left(),
        "languages": languages,
        "un": Language::UN.map(Language::code),
    })
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
        WM_CLOSE => keep(),
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

    /// Opens the choice of a language: the six tiles of the UN's languages, the one of Windows
    /// marked, then a tile for every language, each with its flag, and typing finds one. It starts
    /// WebView2, so it runs only when asked: `LOCALAPPDATA=<folder> cargo test -p ainitshot
    /// choice_of_a_language -- --ignored`. With `SCREENSHOT_PREVIEW=<folder>` it saves the window
    /// at its top and further down, and every flag large.
    #[test]
    #[ignore]
    fn the_choice_of_a_language_shows_the_flags_and_the_rest() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let window = create_window().unwrap();
        WINDOW.with(|cell| *cell.borrow_mut() = Some(Picker { window, controller: None, icons: Vec::new(), first: false }));
        unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
        browser::with_environment(Box::new(move |environment| {
            if let Some(environment) = environment {
                create_controller(environment, window);
            }
        }));
        let ready = pump_until(30, || controller().is_some() && evaluate("document.body.className") == "\"\"");
        let preview = std::env::var_os("SCREENSHOT_PREVIEW").map(PathBuf::from);
        let save = |name: &str| {
            if let Some(folder) = &preview {
                let path = folder.join(name);
                save_page_of(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), &path.to_string_lossy());
            }
        };
        let (mut tiles, mut flags, mut found) = (String::new(), String::new(), String::new());
        if ready {
            pump_until(1, || false);
            tiles = evaluate("['#un', '#all'].map(group => document.querySelectorAll(group + ' .tile').length) + ' ' + document.querySelectorAll('.badge').length");
            flags = evaluate("state.languages.filter(language => !FLAGS[language.code]).map(language => language.code).join()");
            save("languages.png");
            for key in ["d", "e"] {
                evaluate(&format!("document.dispatchEvent(new KeyboardEvent('keydown', {{ key: '{key}' }}))"));
            }
            pump_until(1, || false);
            found = evaluate("document.activeElement.dataset.code");
            save("languages-found.png");
            if preview.is_some() {
                // Further down, a window's height at a time.
                for page in 1.. {
                    let moved = evaluate("(() => { const before = scrollY; scrollBy(0, innerHeight - 60); return scrollY > before; })()");
                    if moved != "true" {
                        break;
                    }
                    save(&format!("languages-{page}.png"));
                }
                evaluate(
                    "document.querySelector('main').innerHTML = '<div id=\"flags\"></div>';
                    const flags = document.querySelector('#flags');
                    flags.style.cssText = 'display: grid; grid-template-columns: repeat(6, 100px); gap: 14px';
                    for (const language of state.languages) {
                        const large = flag(language.code);
                        large.style.cssText = 'width: 100px; height: 67px';
                        flags.append(h('div', {}, large, language.code));
                    }
                    scrollTo(0, 0)",
                );
                save("flags-1.png");
                evaluate("scrollBy(0, innerHeight - 60)");
                save("flags-2.png");
            }
        }
        close();
        browser::close();
        assert!(ready, "the page didn't get its languages");
        assert_eq!(tiles, format!("\"6,{} 1\"", Language::all().count()));
        assert_eq!(flags, "\"\"", "languages without a flag");
        assert_eq!(found, "\"de\"");
    }
}
