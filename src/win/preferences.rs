//! The settings window (decision 7): a page in WebView2 over the settings file, made with the
//! environment that the Google window keeps. On its left are the sections and a search over all
//! settings; a change takes effect at once, once the window has checked it, and goes into the
//! file with its comments. The window shows the settings in force, also when the file has been
//! changed by hand. It is made when opened and let go when closed.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
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
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, HICON, IDC_ARROW, IsIconic,
    LoadCursorW, MINMAXINFO, PostMessageW, RegisterClassExW, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOZORDER, SetForegroundWindow, SetTimer, SetWindowPos, SetWindowTextW, ShowWindow, WA_INACTIVE, WM_ACTIVATE,
    WM_APP, WM_CLOSE, WM_DPICHANGED, WM_GETMINMAXINFO, WM_MOVE, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::{QUIET, app, browser, keys, last_error, models, out_of_sight, shell, tray, update, wide};
use crate::annotation::Tool;
use crate::icons::{self, Part};
use crate::settings::Settings;
use crate::site;
use crate::text::{self, Language, tr};

const CLASS: PCWSTR = w!("ainitShot.settings");
/// The page of the window, in the program itself.
const PAGE: &str = include_str!("preferences.html");
/// The colour of the window until the page is drawn.
const BACKGROUND: u32 = 0x0020_2020;
/// The window's size at the scale of 100%, and the smallest it may become.
const SIZE: (i32, i32) = (920, 640);
const SMALLEST: (i32, i32) = (680, 460);
/// Shows the dialog for a folder, after the event of the page that asked for it.
const WM_PICK_FOLDER: u32 = WM_APP + 1;
/// How long the window waits for the page to save what is still being typed before it closes.
const CLOSE_TIMER: usize = 1;
const CLOSE_WAIT: u32 = 1000;

thread_local! {
    static WINDOW: RefCell<Option<Preferences>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

struct Preferences {
    window: HWND,
    /// Comes a moment after the window, once WebView2 has made it.
    controller: Option<ICoreWebView2Controller>,
    /// Whether the page listens for a new hotkey, while the old one is held back.
    recording: bool,
    /// The folder the dialog for a folder starts in.
    picking: Option<PathBuf>,
    /// The program's icon on the window's title bar and on the taskbar.
    icons: Vec<HICON>,
}

/// Opens the settings window, or brings it to the front if it is open.
pub fn open() {
    // Starting with Windows shows as Windows has it now.
    let changed = app::follow_autostart();
    if let Some(window) = window() {
        if changed {
            refresh();
        }
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
    WINDOW.with(|cell| {
        *cell.borrow_mut() = Some(Preferences { window, controller: None, recording: false, picking: None, icons });
    });
    bring_up(window);
    // The window shows whether someone is signed in to Google as it is now.
    app::check_account();
    browser::with_environment(Box::new(move |environment| match environment {
        Some(environment) => create_controller(environment, window),
        None => without_webview(),
    }));
}

/// WebView2 couldn't start, so the window can't show its page: the settings file opens in its
/// place, as the settings are kept there.
fn without_webview() {
    close();
    if let Err(problem) = shell::open_text(&app::settings_path()) {
        app::notify(not_open(), &problem);
    }
}

/// The settings in force have changed, in the file or in their language: the window shows them,
/// in the language in force.
pub fn refresh() {
    if let Some(window) = window() {
        unsafe { SetWindowTextW(window, wide(&title()).as_ptr()) };
    }
    post(&speaking(json!({
        "type": "settings",
        "settings": in_force(),
        "defaults": serde_json::to_value(Settings::default()).unwrap_or_default(),
    })));
    updates(&update::describe());
    models(&models::describe());
}

/// Where the program stands with updates: the window shows the version installed and the one
/// available, and what came of the last check.
pub fn updates(state: &Value) {
    post(&json!({ "type": "updates", "updates": state }));
}

/// How the download of the model of masking goes: the window shows it in the section of models.
pub fn models(state: &Value) {
    post(&json!({ "type": "models", "models": state }));
}

/// A message to the page with what it needs to speak the language in force: its code, the
/// page's texts in it, whether it is written from right to left, the languages there are, the
/// tools with their keys and their names in it, and the names of keys on the user's keyboard.
fn speaking(mut message: Value) -> Value {
    let language = text::current();
    let languages: Vec<Value> = Language::all().map(|l| json!({ "code": l.code(), "native": l.native() })).collect();
    let tools: Vec<Value> = Tool::ALL.iter().map(|tool| json!({ "key": tool.key().to_string(), "name": tool.name() })).collect();
    if let Value::Object(fields) = &mut message {
        fields.insert("language".into(), language.code().into());
        fields.insert("texts".into(), text::page("settings"));
        fields.insert("rightToLeft".into(), language.right_to_left().into());
        fields.insert("languages".into(), languages.into());
        fields.insert("tools".into(), tools.into());
        fields.insert("names".into(), keys::names());
    }
    message
}

fn not_open() -> &'static str {
    tr("The settings didn't open")
}

/// The window's title in the language in force.
fn title() -> String {
    format!("{} — ainitShot", tr("Settings"))
}

/// Whether someone is signed in to Google in the program's window: the window offers to sign in
/// while nobody is.
pub fn account(signed_in: bool) {
    post(&json!({ "type": "account", "signedIn": signed_in }));
}

/// The file has a mistake: the window says so, and shows the settings still in force.
pub fn problem(text: &str) {
    post(&json!({ "type": "fileProblem", "text": text }));
}

/// Closes the window and lets its page go; the hotkey comes back if the page held it.
pub fn close() {
    let Some(preferences) = WINDOW.with(|cell| cell.borrow_mut().take()) else { return };
    if preferences.recording {
        app::hold_hotkey(false);
    }
    if let Some(controller) = &preferences.controller {
        let _ = unsafe { controller.Close() };
    }
    unsafe { DestroyWindow(preferences.window) };
    for icon in preferences.icons {
        unsafe { DestroyIcon(icon) };
    }
}

fn window() -> Option<HWND> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref().map(|p| p.window))
}

fn controller() -> Option<ICoreWebView2Controller> {
    WINDOW.with(|cell| cell.try_borrow().ok()?.as_ref()?.controller.clone())
}

fn bring_up(window: HWND) {
    unsafe {
        if QUIET {
            ShowWindow(window, SW_SHOWNOACTIVATE);
            return;
        }
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
        let dark = COREWEBVIEW2_COLOR { A: 255, R: 0x20, G: 0x20, B: 0x20 };
        controller.cast::<ICoreWebView2Controller2>()?.SetDefaultBackgroundColor(dark)?;
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetIsZoomControlEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
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
        if let Some(preferences) = cell.borrow_mut().as_mut() {
            preferences.controller = Some(controller.clone());
        }
    });
    Ok(())
}

/// What the page asks for.
fn heard(message: &str) {
    let Ok(message) = serde_json::from_str::<Value>(message) else { return };
    match message["type"].as_str().unwrap_or_default() {
        "ready" => post(&first_look()),
        "save" => {
            let saved = serde_json::from_value::<Settings>(message["settings"].clone())
                .map_err(|e| e.to_string())
                .and_then(app::save_settings);
            post(&match saved {
                Ok(()) => json!({ "type": "saved" }),
                Err(problem) => json!({ "type": "problem", "text": problem }),
            });
        }
        "record" => {
            let on = message["on"].as_bool().unwrap_or(false);
            let was = WINDOW.with(|cell| {
                let mut cell = cell.borrow_mut();
                let preferences = cell.as_mut()?;
                Some(std::mem::replace(&mut preferences.recording, on))
            });
            if was.is_some_and(|was| was != on) {
                app::hold_hotkey(on);
            }
        }
        "pickFolder" => {
            let start = folder(message["folder"].as_str());
            let window = WINDOW.with(|cell| {
                let mut cell = cell.borrow_mut();
                let preferences = cell.as_mut()?;
                preferences.picking = Some(start);
                Some(preferences.window)
            });
            // The dialog waits for the user, and WebView2 doesn't want to wait inside its event.
            if let Some(window) = window {
                unsafe { PostMessageW(window, WM_PICK_FOLDER, 0, 0) };
            }
        }
        "openFolder" => {
            let folder = folder(message["folder"].as_str());
            let _ = std::fs::create_dir_all(&folder);
            if let Err(problem) = shell::open(&folder.to_string_lossy()) {
                app::notify(tr("The folder didn't open"), &problem);
            }
        }
        "openFile" => {
            if let Err(problem) = shell::open_text(&app::settings_path()) {
                app::notify(tr("The settings file didn't open"), &problem);
            }
        }
        // The page has saved what was still being typed: the window closes.
        "closed" => close(),
        "account" => {
            if let Err(problem) = browser::open(browser::ACCOUNT) {
                app::notify(browser::window_not_open(), &problem);
            }
        }
        "checkUpdates" => update::check_now(),
        "installUpdate" => update::install_now(),
        "pauseModels" => models::pause(message["paused"].as_bool().unwrap_or(false)),
        "licenses" => post(&licenses()),
        "openSite" => app::open_site(site::OVERVIEW),
        "openSource" => {
            if let Err(problem) = shell::open(site::SOURCE) {
                app::notify(tr("The site didn't open"), &problem);
            }
        }
        "support" => app::open_site(site::SUPPORT),
        _ => {}
    }
}

/// The program's own license and the open source components it is made of, each with its
/// license, for the section "About". The list is written by `tools/licenses.py`.
fn licenses() -> Value {
    let mut licenses: Value = serde_json::from_str(include_str!("licenses.json")).unwrap_or_default();
    licenses["type"] = "licenses".into();
    licenses["license"] = include_str!("../../LICENSE").into();
    licenses
}

/// What the page needs to show the settings: the settings in force and the defaults, where the
/// file and the default folder are, the tools with their keys, and the icons of the toolbar.
fn first_look() -> Value {
    speaking(json!({
        "type": "init",
        "settings": in_force(),
        "defaults": serde_json::to_value(Settings::default()).unwrap_or_default(),
        "signedIn": app::signed_in(),
        "updates": update::describe(),
        "models": models::describe(),
        "file": app::settings_path().to_string_lossy(),
        "defaultFolder": shell::screenshots_folder().to_string_lossy(),
        "icons": {
            "pencil": parts(icons::PENCIL),
            "save": parts(icons::SAVE),
            "images": parts(icons::IMAGES),
            "question": parts(icons::QUESTION),
        },
        "version": env!("CARGO_PKG_VERSION"),
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

/// The folder for screenshots: the one given, or the default for an empty one.
fn folder(given: Option<&str>) -> PathBuf {
    match given.map(str::trim).filter(|folder| !folder.is_empty()) {
        Some(folder) => PathBuf::from(folder),
        None => shell::screenshots_folder(),
    }
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
        let (place, quiet) = match QUIET {
            true => (out_of_sight(place()), WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE),
            false => (place(), 0),
        };
        let window = CreateWindowExW(
            quiet,
            CLASS,
            wide(&title()).as_ptr(),
            WS_OVERLAPPEDWINDOW,
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

/// Shows the dialog for a folder and tells the page what was chosen.
fn pick_folder(window: HWND) {
    let start = WINDOW.with(|cell| cell.borrow_mut().as_mut()?.picking.take());
    let Some(start) = start else { return };
    if let Some(folder) = shell::pick_folder(window, &start) {
        post(&json!({ "type": "folder", "path": folder.to_string_lossy() }));
    }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_SIZE => {
            if let Some(controller) = controller() {
                let _ = unsafe { controller.SetBounds(client_rect(window)) };
            }
        }
        WM_MOVE => {
            // Lists and the colour picker of the page open in the right place.
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
        WM_GETMINMAXINFO => {
            let dpi = unsafe { GetDpiForWindow(window) }.max(96) as i32;
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            info.ptMinTrackSize = POINT { x: SMALLEST.0 * dpi / 96, y: SMALLEST.1 * dpi / 96 };
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam as *const RECT) };
            let (w, h) = (suggested.right - suggested.left, suggested.bottom - suggested.top);
            unsafe { SetWindowPos(window, null_mut(), suggested.left, suggested.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE) };
        }
        WM_PICK_FOLDER => pick_folder(window),
        WM_CLOSE => {
            // The page saves what is still being typed first, and then asks to close; without
            // an answer the window closes all the same.
            if controller().is_some() {
                post(&json!({ "type": "close" }));
                unsafe { SetTimer(window, CLOSE_TIMER, CLOSE_WAIT, None) };
            } else {
                close();
            }
        }
        WM_TIMER if wparam == CLOSE_TIMER => close(),
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

    use super::super::browser::tests::{evaluate_on, pump_until, save_page_of};
    use super::*;

    /// The list of components holds what the program is made of, at the versions it is built
    /// with, each with the text of its license: after the dependencies change, `tools/licenses.py`
    /// writes it again.
    #[test]
    fn every_component_comes_with_its_license() {
        let licenses = licenses();
        let texts = licenses["texts"].as_array().unwrap().len();
        let components = licenses["components"].as_array().unwrap();
        let lock: toml::Table = toml::from_str(include_str!("../../Cargo.lock")).unwrap();
        let locked: Vec<(&str, &str)> = lock["package"]
            .as_array()
            .unwrap()
            .iter()
            .map(|package| (package["name"].as_str().unwrap(), package["version"].as_str().unwrap()))
            .collect();
        let manifest: toml::Table = toml::from_str(include_str!("../../Cargo.toml")).unwrap();
        for name in manifest["dependencies"].as_table().unwrap().keys() {
            assert!(components.iter().any(|c| c["name"] == name.as_str()), "{name} isn't listed");
        }
        for component in components {
            let numbers = component["texts"].as_array().unwrap();
            assert!(!numbers.is_empty() && numbers.iter().all(|n| (n.as_u64().unwrap() as usize) < texts), "{component}");
            let (name, version) = (component["name"].as_str().unwrap(), component["version"].as_str().unwrap());
            let crate_of_ours = component["downloaded"].is_null() && !name.contains(' ');
            assert!(!crate_of_ours || locked.contains(&(name, version)), "{name} {version} isn't the version built");
        }
        assert!(licenses["license"].as_str().unwrap().contains("GNU GENERAL PUBLIC LICENSE"));
    }

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
        let mut russian: Vec<&str> = catalog["settings"].as_object().unwrap().keys().map(String::as_str).collect();
        page.sort_unstable();
        russian.sort_unstable();
        assert!(page.len() > 150, "{}", page.len());
        assert_eq!(page, russian);
    }

    /// What a script returns on the settings page, as JSON.
    fn evaluate(script: &str) -> String {
        evaluate_on(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), script)
    }

    /// Saves what the settings page shows as a PNG file.
    fn save_page(path: &std::path::Path) {
        save_page_of(&unsafe { controller().unwrap().CoreWebView2() }.unwrap(), &path.to_string_lossy());
    }

    /// Shows the settings window in every language the program speaks: every text of the page has
    /// its translation, those its elements show and those its script writes, and the page says
    /// which language it is in. With `SCREENSHOT_PREVIEW=<folder>` it saves the first section and
    /// a query button opened in each: `LOCALAPPDATA=<folder> cargo test -p ainitshot speaks_every
    /// -- --ignored`.
    #[test]
    #[ignore]
    fn the_settings_window_speaks_every_language() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        open();
        let ready = pump_until(30, || controller().is_some() && evaluate("document.body.className") == "\"\"");
        let preview = std::env::var_os("SCREENSHOT_PREVIEW").map(PathBuf::from);
        let mut seen = Vec::new();
        if ready {
            for language in Language::all().chain([Language::RU]) {
                text::set(language);
                refresh();
                pump_until(2, || false);
                let missing = "[...new Set([...document.querySelectorAll('[data-t],[data-t-title],[data-t-aria],[data-t-placeholder],[data-k]')]
                    .flatMap(e => [e.dataset.t, e.dataset.tTitle, e.dataset.tAria, e.dataset.tPlaceholder, e.dataset.k])
                    .filter(Boolean).concat(Object.keys(TEXT)))]
                    .filter(key => !(TEXT[key] && (state.language === 'en' || state.texts[key])))";
                seen.push(evaluate(&format!("document.documentElement.lang + ' ' + JSON.stringify({missing})")));
                if let Some(folder) = &preview {
                    evaluate("document.querySelector('nav [data-section=general]').click()");
                    pump_until(1, || false);
                    save_page(&folder.join(format!("settings-{}.png", language.code())));
                    evaluate("document.querySelector('nav [data-section=buttons]').click(); document.querySelector('.card:not(.open) .card-head').click()");
                    pump_until(1, || false);
                    save_page(&folder.join(format!("settings-{}-button.png", language.code())));
                }
            }
        }
        close();
        browser::close();
        assert!(ready, "the page didn't get its settings");
        let expected: Vec<String> = Language::all().chain([Language::RU]).map(|l| format!("\"{} []\"", l.code())).collect();
        assert_eq!(seen, expected);
    }

    /// Saves pictures of the section "About" into the folder `SCREENSHOT_PREVIEW`, in Russian and
    /// in Arabic: the latest version, installed by itself; a new version while updates are
    /// installed by hand; and the page of the components. The window opens out of sight:
    /// `LOCALAPPDATA=<folder> SCREENSHOT_PREVIEW=<folder> cargo test -p ainitshot
    /// about_section_in_pictures -- --ignored`.
    #[test]
    #[ignore]
    fn the_about_section_in_pictures() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let folder = PathBuf::from(std::env::var_os("SCREENSHOT_PREVIEW").expect("a folder for the pictures"));
        open();
        let ready = pump_until(30, || controller().is_some() && evaluate("document.body.className") == "\"\"");
        let mut saved = Vec::new();
        // The last check was this morning. The page keeps what a script declares, so each script
        // declares it in a block of its own.
        let checked = "const now = new Date(), two = n => String(n).padStart(2, '0'); \
                       const checked = `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())} 09:41`;";
        let mut save = |name: String| {
            pump_until(1, || false);
            let path = folder.join(name);
            save_page(&path);
            saved.push(path.exists());
        };
        if ready {
            for language in [Language::RU, Language::AR] {
                text::set(language);
                refresh();
                pump_until(2, || false);
                let code = language.code();
                evaluate("document.querySelector('nav [data-section=about]').click()");
                pump_until(5, || evaluate("document.querySelectorAll('#components .component').length") != "0");
                evaluate(&format!("{{ {checked} state.draft.autoupdate = true; renderSwitch('autoupdate', true); \
                    heard({{ type: 'updates', updates: {{ current: state.version, state: 'latest', checked }} }}) }}"));
                save(format!("about-{code}.png"));
                evaluate(&format!("{{ {checked} state.draft.autoupdate = false; renderSwitch('autoupdate', false); \
                    heard({{ type: 'updates', updates: {{ current: state.version, state: 'available', version: '0.3.0', checked }} }}) }}"));
                save(format!("about-{code}-update.png"));
                evaluate("aboutPage('components')");
                save(format!("about-{code}-components.png"));
                evaluate("aboutPage('main')");
            }
        }
        close();
        browser::close();
        assert!(ready, "the page didn't get its settings");
        assert_eq!(saved, [true; 6]);
    }

    /// Opens the settings window with the default settings and goes through its sections and
    /// its search. It shows the window and starts WebView2, whose Google window goes online, so
    /// it runs only when asked: `LOCALAPPDATA=<folder> cargo test -p ainitshot settings_window
    /// -- --ignored`. With `SCREENSHOT_PREVIEW=<folder>` it also saves what each page shows.
    #[test]
    #[ignore]
    fn the_settings_window_shows_every_section_and_finds_settings() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        open();
        let ready = pump_until(30, || controller().is_some() && evaluate("document.body.className") == "\"\"");
        let preview = std::env::var_os("SCREENSHOT_PREVIEW").map(PathBuf::from);
        let look = |name: &str| {
            pump_until(1, || false);
            if let Some(folder) = &preview {
                save_page(&folder.join(format!("settings-{name}.png")));
            }
        };
        let mut seen = Vec::new();
        if ready {
            // Nobody is signed in to Google in a fresh profile: the window invites to sign in. The
            // test has no program behind the window to remember it, so it is asked again.
            app::check_account();
            pump_until(15, || evaluate("state.signedIn") != "null");
            seen.push(evaluate("state.signedIn + ' ' + document.querySelector('#sign-in-callout').checkVisibility()"));
            seen.push(evaluate("state.draft.hotkey + ' ' + [...document.querySelectorAll('#hotkey kbd')].map(k => k.textContent).join('+')"));
            look("general");
            for section in ["drawing", "panels", "saving", "buttons", "account"] {
                evaluate(&format!("document.querySelector('nav [data-section={section}]').click()"));
                look(section);
            }
            evaluate("document.querySelector('nav [data-section=buttons]').click(); document.querySelector('.card-head').click()");
            look("button-open");
            seen.push(evaluate("document.querySelectorAll('.card').length"));
            seen.push(evaluate("document.querySelector('#reset-buttons').checkVisibility()"));
            evaluate("{ const s = document.querySelector('#search'); s.value = 'папк'; s.dispatchEvent(new Event('input')); }");
            look("search");
            seen.push(evaluate("document.querySelector('nav [data-section=saving] .count').textContent"));
            evaluate("{ const s = document.querySelector('#search'); s.value = 'жираф'; s.dispatchEvent(new Event('input')); }");
            look("nothing");
            seen.push(evaluate("document.body.classList.contains('nothing')"));
            evaluate("clearSearch()");
            // About: the page asks the program for the licenses as the section opens. The license
            // and the components are pages of their own under the section's title: every
            // component comes, a click on one shows its license, and the title leads back.
            evaluate("document.querySelector('nav [data-section=about]').click()");
            pump_until(5, || evaluate("document.querySelectorAll('#components .component').length") != "0");
            look("about");
            let pages = "['#about-main', '#license-page', '#components-page'].map(p => document.querySelector(p).checkVisibility() ? 1 : 0).join('')";
            evaluate("document.querySelector('#about-components').click()");
            seen.push(evaluate(&format!("{pages} + ' ' + document.querySelector('#about-sub').textContent")));
            seen.push(evaluate("document.querySelectorAll('#components .component').length + ' ' + document.querySelectorAll('#components .group').length"));
            evaluate("document.querySelector('#components .component').click()");
            seen.push(evaluate("document.querySelector('#components .component-text').textContent.includes('Permission is hereby granted')"));
            look("about-components");
            evaluate("document.querySelector('#about-home').click(); document.querySelector('#about-license').click()");
            seen.push(evaluate(&format!("{pages} + ' ' + document.querySelector('#license-text').textContent.includes('GNU GENERAL PUBLIC LICENSE')")));
            evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))");
            seen.push(evaluate(pages));
            // What the page sends the program is kept, without the program behind it.
            evaluate("window.told = []; chrome.webview.postMessage = m => told.push(JSON.parse(m))");
            let told = |what: &str| evaluate(&format!("JSON.stringify(told.map(m => {what}))"));
            evaluate("document.querySelector('#autostart').click()");
            seen.push(told("m.type + ':' + m.settings.autostart"));
            // Updates: the switch is saved as the other settings are, and the button asks the
            // program to check, or to install the version available.
            evaluate("told.length = 0; document.querySelector('#autoupdate').click()");
            seen.push(told("m.type + ':' + m.settings.autoupdate"));
            evaluate("told.length = 0; heard({ type: 'updates', updates: { current: '0.1.0', state: 'latest', checked: '2026-10-25 14:05' } })");
            evaluate("document.querySelector('#updates-button').click()");
            seen.push(told("m.type"));
            evaluate("told.length = 0; heard({ type: 'updates', updates: { current: '0.1.0', state: 'available', version: '0.2.0' } })");
            evaluate("document.querySelector('#updates-button').click()");
            seen.push(told("m.type"));
            seen.push(evaluate("document.querySelector('#updates-title').textContent + ' | ' + document.querySelector('#updates-button').textContent"));
            // The card of support and the links ask the program to open the site and the source.
            evaluate("told.length = 0; ['#support', '#about-site', '#about-source'].forEach(s => document.querySelector(s).click())");
            seen.push(told("m.type"));
            // The model of masking: its switch is saved as the other settings are; its download
            // shows how far it has come, and its button pauses it.
            evaluate("document.querySelector('nav [data-section=models]').click()");
            evaluate("told.length = 0; document.querySelector('#privacy-filter').click()");
            seen.push(told("m.type + ':' + m.settings.privacy_filter"));
            evaluate("heard({ type: 'models', models: { state: 'downloading', problem: null, done: 380000000, total: 951000000, speed: 12300000 } })");
            look("models");
            seen.push(evaluate("document.querySelector('#filter-state').textContent + ' | ' + document.querySelector('#filter-fill').style.width"));
            evaluate("told.length = 0; document.querySelector('#filter-pause').click()");
            seen.push(told("m.type + ':' + m.paused"));
            evaluate("document.querySelector('nav [data-section=about]').click()");
            look("updates");
            evaluate("told.length = 0; document.querySelector('#hotkey').click()");
            evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyS', key: 's', keyCode: 83, ctrlKey: true, shiftKey: true }))");
            seen.push(told("m.type + ':' + (m.type === 'save' ? m.settings.hotkey : m.on)"));
            evaluate("told.length = 0; heard({ type: 'saved' }); heard({ type: 'saved' })");
            // A bare letter would get in the way of typing: the field keeps listening.
            evaluate("document.querySelector('#hotkey').click()");
            evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyQ', key: 'q', keyCode: 81 }))");
            seen.push(evaluate("document.querySelector('#hotkey-error').textContent"));
            evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { code: 'Escape', key: 'Escape' }))");
            seen.push(told("m.type + ':' + m.on"));
            // In any layout the key left of 1 is ~, ^ in German too, and a letter is the key with
            // it, Z in German where English has Y.
            evaluate("told.length = 0; document.querySelector('#hotkey').click()");
            evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { code: 'Backquote', key: 'Dead', keyCode: 220, altKey: true }))");
            evaluate("heard({ type: 'saved' }); document.querySelector('#hotkey').click()");
            evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyY', key: 'z', keyCode: 90, ctrlKey: true, altKey: true }))");
            evaluate("heard({ type: 'saved' })");
            seen.push(evaluate("JSON.stringify(told.filter(m => m.type === 'save').map(m => m.settings.hotkey))"));
            // A tool's key isn't taken by a button, and nothing is saved.
            evaluate("told.length = 0; document.querySelector('.card .key-field').dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyP', key: 'p', keyCode: 80, bubbles: true }))");
            seen.push(evaluate("document.querySelector('.card .key-field + .error').textContent + ' ' + told.length"));
            // A button's key is the letter on the key too: Z in French, where English has W.
            evaluate("document.querySelector('.card .key-field').dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW', key: 'z', keyCode: 90, bubbles: true }))");
            seen.push(told("m.settings.buttons[0].key"));
            // A colour removed goes, and comes back with «Вернуть».
            evaluate("told.length = 0; document.querySelector('.swatch[data-index=\"1\"] .remove').click()");
            evaluate("document.querySelector('#toast .btn').click()");
            seen.push(told("m.settings.colors.length"));
            // Saving: by default the screenshot goes straight into the folder and into the
            // clipboard too; the switches are saved as the other settings are.
            evaluate("told.length = 0; document.querySelector('#ask').click(); document.querySelector('#save-copy').click()");
            seen.push(told("m.settings.save.ask + '/' + m.settings.save.copy"));
            // An empty name isn't saved; a name is, a moment after the typing.
            evaluate("told.length = 0; { const n = document.querySelector('#name'); n.value = '  '; n.dispatchEvent(new Event('input')); }");
            pump_until(1, || false);
            seen.push(evaluate("document.querySelector('#name-error').textContent + ' ' + told.length"));
            evaluate("{ const n = document.querySelector('#name'); n.value = 'Снимок {date}'; n.dispatchEvent(new Event('input')); }");
            pump_until(1, || false);
            seen.push(told("m.settings.save.name"));
            look("changed");
            // The query buttons go under the selection too, at its right end: the two panels
            // stand in a row, and change places.
            evaluate("told.length = 0; document.querySelector('nav [data-section=panels]').click()");
            evaluate("document.querySelector('[data-panel=queries] .spot.bottom-right').click()");
            seen.push(told("m.settings.panels.queries.side + '/' + m.settings.panels.queries.align"));
            seen.push(evaluate("document.querySelector('#panels-order').checkVisibility()"));
            evaluate("told.length = 0; document.querySelector('#panels-swap').click()");
            seen.push(told("m.settings.panels.first"));
            look("panels-row");
        }
        if !ready && controller().is_some() {
            eprintln!("className {}", evaluate("document.body.className"));
            eprintln!("webview {}", evaluate("typeof window.chrome.webview"));
            eprintln!("state {}", evaluate("JSON.stringify({ inForce: !!state.inForce, icons: Object.keys(state.icons) })"));
            eprintln!("try {}", evaluate("try { init(window.__init || {}); 'ok' } catch (e) { String(e) + ' ' + e.stack }"));
        }
        eprintln!("controller {}", controller().is_some());
        close();
        browser::close();
        assert!(ready, "the page didn't get its settings");
        // «папк» is in the folder's setting and in what asking where to save says of the folder.
        assert_eq!(
            seen,
            [
                "\"false true\"",
                "\"PrintScreen PrtSc\"",
                "3",
                "false",
                "\"2\"",
                "true",
                "\"001 Открытые компоненты\"",
                "\"122 2\"",
                "true",
                "\"010 true\"",
                "\"100\"",
                r#""[\"save:true\"]""#,
                r#""[\"save:false\"]""#,
                r#""[\"checkUpdates\"]""#,
                r#""[\"installUpdate\"]""#,
                "\"Доступна версия 0.2.0 | Обновить до 0.2.0\"",
                r#""[\"support\",\"openSite\",\"openSource\"]""#,
                r#""[\"save:false\"]""#,
                "\"39% · 380 из 951 МБ · 12,3 МБ/с | 40%\"",
                r#""[\"pauseModels:true\"]""#,
                r#""[\"record:true\",\"record:false\",\"save:Ctrl+Shift+S\"]""#,
                // The keys read from left to right in every language.
                "\"\u{2066}Q\u{2069} мешала бы печатать: добавь Ctrl, Alt или Win.\"",
                r#""[\"record:true\",\"record:false\"]""#,
                r#""[\"Alt+~\",\"Ctrl+Alt+Z\"]""#,
                "\"Клавиша P уже у инструмента «Карандаш». 0\"",
                r#""[\"Z\"]""#,
                "\"[7,8]\"",
                r#""[\"true/true\",\"true/false\"]""#,
                "\"Имя не может быть пустым. 0\"",
                r#""[\"Снимок {date}\"]""#,
                r#""[\"bottom/right\"]""#,
                "true",
                r#""[\"queries\"]""#,
            ]
        );
    }
}
