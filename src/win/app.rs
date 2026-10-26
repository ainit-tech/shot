//! The program's life: one process in the background with a hidden window that receives the
//! hotkey, the clicks on the icon in the notification area and a tick to look at the settings.
//! As it starts, the program offers to sign in to Google if nobody is signed in in its window:
//! Google serves someone signed in better.

use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::{Duration, SystemTime};

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows_sys::Win32::System::Console::{CTRL_CLOSE_EVENT, SetConsoleCtrlHandler};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Shell::NIN_BALLOONUSERCLICK;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
    GetMessageW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MESSAGEBOX_STYLE, MSG, MessageBoxW, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetTimer, TranslateMessage, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW,
};
use windows_sys::core::BOOL;
use windows_sys::w;

use super::hotkey::{self, Hotkey};
use super::tray::{self, Tray};
use super::{browser, guide, invite, keys, languages, last_error, models, overlay, preferences, shell, update, wide};
use crate::settings::{self, Settings};
use crate::site;
use crate::text::{self, Language, tr};

/// What another start of the program with `--first-run` asks of the copy running: to show the
/// windows of the first start.
const FIRST_RUN: u32 = WM_APP + 12;

/// How often the settings file is looked at, in milliseconds.
const SETTINGS_TICK: u32 = 1000;
const SETTINGS_TIMER: usize = 1;

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    /// The screenshot the last notification told of: a click on it shows the file in its folder.
    static SAVED: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// The message Explorer sends to every window when it starts again: the icon has to be added
/// anew.
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

/// The program's hidden window, for the console's events, which come on a thread of their own,
/// and whether the program has let everything go.
static MAIN_WINDOW: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
static ENDED: AtomicBool = AtomicBool::new(false);

/// Ctrl+C, or the console closing, where the program runs while it is developed, ends the program
/// as "Выход" does: WebView2 is let go properly, so that it keeps the sign-in to Google intact.
unsafe extern "system" fn console_event(event: u32) -> BOOL {
    let window = MAIN_WINDOW.load(Ordering::Acquire);
    if window.is_null() {
        return 0;
    }
    unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
    if event == CTRL_CLOSE_EVENT {
        // Windows ends the process as soon as this returns, so it waits for the program first.
        for _ in 0..40 {
            if ENDED.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    1
}

/// The settings in force and what they have set up in Windows.
struct State {
    window: HWND,
    path: PathBuf,
    settings: Rc<Settings>,
    /// When the file was last read, and a newer time seen once: the file is read when that time
    /// holds for a whole tick, so that a file still being written isn't read.
    read: Option<SystemTime>,
    seen: Option<SystemTime>,
    hotkey: Option<Hotkey>,
    /// Whether someone is signed in to Google in the program's window, once known.
    signed_in: Option<bool>,
}

/// Saves settings from the settings window and puts them in force at once. A new hotkey is
/// taken first: if another program has it, the settings stay as they were and the window hears
/// why. The file is written whole, with its comments, and the tick doesn't read it again. In
/// another language the standard query buttons and the file's name speak it too, and the windows
/// of the program show the settings in it.
pub fn save_settings(new: Settings) -> Result<(), String> {
    new.check()?;
    let Some((window, path, old, hotkey)) =
        STATE.with(|s| s.borrow().as_ref().map(|s| (s.window, s.path.clone(), s.settings.clone(), s.hotkey)))
    else {
        return Err(tr("The program isn't ready yet.").into());
    };
    let mut registered = hotkey;
    if new.hotkey != old.hotkey {
        let taken = hotkey::parse(&new.hotkey)?;
        if !hotkey::take(window, taken) {
            if let Some(old) = hotkey {
                hotkey::take(window, old);
            }
            return Err(taken_by_another(&new.hotkey));
        }
        registered = Some(taken);
    }
    let (was, language) = (text::current(), new.language.resolve(shell::windows_language()));
    let new = new.speaking(was, language);
    text::set(language);
    // Written beside it first, so that a half-written file is never read.
    let fresh = path.with_extension("toml.new");
    fs::write(&fresh, new.to_file()).and_then(|()| fs::rename(&fresh, &path)).map_err(|e| cannot_write(&path, &e))?;
    if new.autostart != old.autostart
        && let Err(problem) = shell::set_autostart(new.autostart)
    {
        notify(tr("Starting with Windows hasn't changed"), &problem);
    }
    if new.privacy_filter != old.privacy_filter {
        models::want(new.privacy_filter);
    }
    let now = modified(&path);
    change_state(|state| {
        state.settings = Rc::new(new);
        state.hotkey = registered;
        state.read = now;
        state.seen = now;
    });
    if language != was {
        preferences::refresh();
    }
    guide::refresh();
    Ok(())
}

fn taken_by_another(hotkey: &str) -> String {
    tr("Another program has taken {}.")
        .replace("{}", hotkey)
}

fn cannot_write(path: &std::path::Path, error: &io::Error) -> String {
    tr("Couldn't write {path}: {error}.")
        .replace("{path}", &path.display().to_string())
        .replace("{error}", &error.to_string())
}

/// Takes the hotkey away while the settings window listens for a new one, so that pressing the
/// old one doesn't freeze the screen, and gives it back after.
pub fn hold_hotkey(held: bool) {
    let Some((window, hotkey)) = STATE.with(|s| s.borrow().as_ref().map(|s| (s.window, s.hotkey))) else { return };
    hotkey::release(window);
    if !held && let Some(hotkey) = hotkey {
        hotkey::take(window, hotkey);
    }
}

/// Takes the hotkey of the settings again after the choice of the keys, which held it back; tells
/// the user if another program has it.
pub fn retake_hotkey() {
    let window = main_window();
    if window.is_null() {
        return;
    }
    hotkey::release(window);
    match register(window, &settings().hotkey) {
        Ok(hotkey) => change_state(|state| state.hotkey = Some(hotkey)),
        Err(problem) => {
            change_state(|state| state.hotkey = None);
            let click = tr("A click on the icon takes a screenshot.");
            notify(tr("The hotkey doesn't work"), &format!("{problem} {click}"));
        }
    }
}

/// The program's hidden window, which holds the hotkey; none until the program has started.
pub fn main_window() -> HWND {
    MAIN_WINDOW.load(Ordering::Acquire)
}

/// Finds out again whether someone is signed in to Google in the program's window, for the
/// settings window: after the window closes, where the user may have signed in or out, and as
/// the settings window opens.
pub fn check_account() {
    browser::signed_in(Box::new(account_known));
}

/// Whether someone is signed in to Google in the program's window, once known.
pub fn signed_in() -> Option<bool> {
    STATE.with(|s| s.borrow().as_ref().and_then(|s| s.signed_in))
}

fn account_known(signed_in: bool) {
    change_state(|state| state.signed_in = Some(signed_in));
    preferences::account(signed_in);
    guide::account(signed_in);
}

/// The settings file.
pub fn settings_path() -> PathBuf {
    STATE.with(|s| s.borrow().as_ref().map(|s| s.path.clone())).unwrap_or_else(shell::settings_path)
}

/// The settings in force.
pub fn settings() -> Rc<Settings> {
    STATE.with(|state| state.borrow().as_ref().map(|s| s.settings.clone())).unwrap_or_default()
}

fn change_state(change: impl FnOnce(&mut State)) {
    STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            change(state);
        }
    });
}

/// Opens a page of the program's site in the user's browser, in the program's language where the
/// site speaks it.
pub fn open_site(path: &str) {
    if let Err(problem) = shell::open(&site::page(text::current(), path)) {
        notify(tr("The site didn't open"), &problem);
    }
}

/// Tells the user of a problem next to the icon in the notification area.
pub fn notify(title: &str, text: &str) {
    show_notice(title, text, true, None);
}

/// Tells the user good news next to the icon, quietly.
pub fn announce(title: &str, text: &str) {
    show_notice(title, text, false, None);
}

/// Tells the user where a screenshot is saved, and whether it is copied too; a click on the
/// notification shows the file in its folder.
pub fn saved(path: &Path, copied: bool) {
    let title = if copied { tr("Screenshot saved and copied") } else { tr("Screenshot saved") };
    show_notice(title, &path.display().to_string(), false, Some(path.to_path_buf()));
}

fn show_notice(title: &str, text: &str, problem: bool, file: Option<PathBuf>) {
    eprintln!("{title}: {text}");
    SAVED.set(file);
    TRAY.with(|tray| {
        if let Some(tray) = &*tray.borrow() {
            tray.notify(title, text, problem);
        }
    });
}

pub fn run() -> io::Result<()> {
    unsafe {
        // The program works in real pixels on every monitor, whatever its scale.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // The program's browser window, WebView2, works through COM on this thread.
        CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        // The program speaks the language of Windows until the settings say otherwise.
        let windows = shell::windows_language();
        text::set(windows);

        // `--first-run` shows the windows of the first start again, to see them: the language, the
        // keys and how to use the program. Nothing changes in the settings unless one chooses there.
        let first_run = std::env::args().any(|argument| argument == "--first-run");

        // One copy of the program at a time; the handle stays open while the process runs. With
        // `--first-run`, the copy running shows the windows itself.
        let _single = CreateMutexW(null(), 0, w!("Local\\ainitShot"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let running = FindWindowW(w!("ainitShot.main"), null());
            if first_run && !running.is_null() {
                AllowSetForegroundWindow(ASFW_ANY);
                PostMessageW(running, FIRST_RUN, 0, 0);
            } else {
                tell(tr("ainitShot is already running."), MB_ICONINFORMATION);
            }
            return Ok(());
        }

        let module = GetModuleHandleW(null());
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            lpszClassName: w!("ainitShot.main"),
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(last_error());
        }
        let window =
            CreateWindowExW(0, w!("ainitShot.main"), w!("ainitShot"), 0, 0, 0, 0, 0, null_mut(), null_mut(), module, null());
        if window.is_null() {
            return Err(last_error());
        }
        TASKBAR_CREATED.get_or_init(|| RegisterWindowMessageW(w!("TaskbarCreated")));
        MAIN_WINDOW.store(window, Ordering::Release);
        SetConsoleCtrlHandler(Some(console_event), 1);

        let path = shell::settings_path();
        // Without a settings file this is the program's first start.
        let first = !path.exists();
        let (initial, problem) = first_settings(&path, windows, shell::installer_language());
        // A settings file from before the program spoke several languages has none chosen.
        let unchosen = !first && fs::read_to_string(&path).is_ok_and(|text| !settings::chooses_language(&text));
        text::set(initial.language.resolve(windows));
        let read = modified(&path);
        STATE.with(|state| {
            *state.borrow_mut() =
                Some(State { window, path, settings: Rc::new(initial), read, seen: read, hotkey: None, signed_in: None });
        });

        overlay::create(module)?;
        let tray = Tray::add(window)?;
        TRAY.with(|t| *t.borrow_mut() = Some(tray));
        // The installer of an update starts the new version with this, which says so.
        if std::env::args().any(|argument| argument == "--updated") {
            let version = crate::update::Version::current().to_string();
            announce(tr("ainitShot is updated"), &tr("Version {version} is installed.").replace("{version}", &version));
        }
        update::start(window);

        if let Some(problem) = problem {
            let meanwhile = tr("The default settings are in force meanwhile.");
            notify(settings_mistake(), &format!("{problem} {meanwhile}"));
        }
        let settings = settings();
        models::start(window, settings.privacy_filter);
        match register(window, &settings.hotkey) {
            Ok(hotkey) => change_state(|state| state.hotkey = Some(hotkey)),
            // On the first start the choice of the keys says it, and offers others.
            Err(problem) if first || first_run => eprintln!("{problem}"),
            Err(problem) => {
                let click = tr("A click on the icon takes a screenshot.");
                notify(tr("The hotkey doesn't work"), &format!("{problem} {click}"));
            }
        }
        follow_autostart();
        SetTimer(window, SETTINGS_TIMER, SETTINGS_TICK, None);
        // Pages of queries left from the last run hold screenshots.
        shell::remove_pages(Duration::ZERO);
        // The first question goes to a page ready in the background, as every one after it does.
        browser::prepare();
        // The first time, the program asks which language to speak and which keys take a
        // screenshot, and then shows how to use it; a program that has spoken before only asks the
        // language, if it was never chosen.
        if first || unchosen || first_run {
            languages::open(first || first_run);
        }
        // Google serves someone signed in better: if nobody is, the program invites to sign in,
        // once the presentation is closed if it is open.
        browser::signed_in(Box::new(|signed_in| {
            account_known(signed_in);
            if signed_in {
                eprintln!("Signed in to Google.");
            } else {
                eprintln!("Not signed in to Google: the program invites to sign in.");
                if guide::is_open() || languages::is_open() || keys::is_open() {
                    guide::invite_when_closed();
                } else {
                    invite::open();
                }
            }
        }));

        let mut message = MSG::default();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    ENDED.store(true, Ordering::Release);
    Ok(())
}

/// Starting with Windows as Windows has it: the installer, Task Manager or Settings may have
/// changed it since the settings were written, and the settings, with their file, follow it.
/// Whether they changed.
pub fn follow_autostart() -> bool {
    let on = shell::starts_with_windows();
    let Some((path, settings)) = STATE.with(|s| s.borrow().as_ref().map(|s| (s.path.clone(), s.settings.clone()))) else {
        return false;
    };
    if settings.autostart == on {
        return false;
    }
    let new = Settings { autostart: on, ..(*settings).clone() };
    // Written beside it first, so that a half-written file is never read.
    let fresh = path.with_extension("toml.new");
    if let Err(error) = fs::write(&fresh, new.to_file()).and_then(|()| fs::rename(&fresh, &path)) {
        eprintln!("{}", cannot_write(&path, &error));
    }
    let now = modified(&path);
    change_state(|state| {
        state.settings = Rc::new(new);
        state.read = now;
        state.seen = now;
    });
    true
}

/// The settings at the start. The file is written with the defaults if there is none; if it has
/// a mistake, the defaults are used and the mistake is returned too. After an installation in
/// another language than that of Windows, settings that haven't chosen a language of their own
/// speak the installation's (decision 11).
fn first_settings(path: &PathBuf, windows: Language, installed: Option<Language>) -> (Settings, Option<String>) {
    if !path.exists() {
        // The installer turns starting with Windows on unless told not to; the first settings
        // keep what it chose.
        let first = Settings { autostart: shell::starts_with_windows(), ..Settings::default() };
        let written = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(path, first.to_file()));
        if let Err(error) = written {
            return (Settings::default(), Some(cannot_write(path, &error)));
        }
    }
    let settings = match fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|text| Settings::parse(&text)) {
        Ok(settings) => settings,
        Err(problem) => return (Settings::default(), Some(problem)),
    };
    let moved = installed.map(|language| settings.clone().installed_in(language, windows)).filter(|moved| *moved != settings);
    let Some(moved) = moved else { return (settings, None) };
    // The file speaks the new language too.
    text::set(moved.language.resolve(windows));
    match fs::write(path, moved.to_file()) {
        Ok(()) => (moved, None),
        Err(error) => (moved, Some(cannot_write(path, &error))),
    }
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Rereads the settings file once it has changed and stayed so for a tick.
fn reread() {
    let Some((path, read, seen)) = STATE.with(|s| s.borrow().as_ref().map(|s| (s.path.clone(), s.read, s.seen))) else {
        return;
    };
    let now = modified(&path);
    if now == read {
        return;
    }
    if now != seen {
        change_state(|state| state.seen = now);
        return;
    }
    change_state(|state| state.read = now);
    match fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| Settings::parse(&text)) {
        Ok(settings) => {
            apply(settings);
            preferences::refresh();
        }
        Err(problem) => {
            let meanwhile = tr("The settings from before are in force meanwhile.");
            notify(settings_mistake(), &format!("{problem} {meanwhile}"));
            preferences::problem(&problem);
        }
    }
}

fn settings_mistake() -> &'static str {
    tr("The settings have a mistake")
}

/// Puts new settings in force: the language, the hotkey and starting with Windows change at
/// once, the rest with the next screenshot.
fn apply(new: Settings) {
    let Some((window, old, hotkey)) = STATE.with(|s| s.borrow().as_ref().map(|s| (s.window, s.settings.clone(), s.hotkey)))
    else {
        return;
    };
    text::set(new.language.resolve(shell::windows_language()));
    let mut registered = hotkey;
    if new.hotkey != old.hotkey || hotkey.is_none() {
        match register(window, &new.hotkey) {
            Ok(hotkey) => registered = Some(hotkey),
            Err(problem) => {
                notify(tr("The hotkey hasn't changed"), &problem);
                registered = hotkey.filter(|&old| hotkey::take(window, old));
            }
        }
    }
    if new.autostart != old.autostart
        && let Err(problem) = shell::set_autostart(new.autostart)
    {
        notify(tr("Starting with Windows hasn't changed"), &problem);
    }
    if new.privacy_filter != old.privacy_filter {
        models::want(new.privacy_filter);
    }
    change_state(|state| {
        state.settings = Rc::new(new);
        state.hotkey = registered;
    });
    guide::refresh();
}

fn register(window: HWND, text: &str) -> Result<Hotkey, String> {
    let hotkey = hotkey::parse(text)?;
    if !hotkey::take(window, hotkey) {
        return Err(taken_by_another(text));
    }
    Ok(hotkey)
}

/// Tells the user about a problem that stops the program. It has no window of its own for that,
/// so a message box is used.
pub fn report(text: &str) {
    tell(text, MB_ICONERROR);
}

fn tell(text: &str, icon: MESSAGEBOX_STYLE) {
    eprintln!("{text}");
    let text = wide(text);
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), w!("ainitShot"), MB_OK | icon) };
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_HOTKEY => overlay::open(),
        WM_TIMER if wparam == SETTINGS_TIMER => {
            reread();
            hotkey::follow(window);
        }
        WM_TIMER if wparam == hotkey::TIMER => hotkey::follow(window),
        WM_TIMER if wparam == update::TIMER => update::tick(),
        update::MESSAGE => update::heard(lparam),
        models::MESSAGE => models::heard(wparam != 0),
        FIRST_RUN => {
            if !languages::is_open() && !keys::is_open() {
                languages::open(true);
            }
        }
        tray::MESSAGE => match lparam as u32 {
            WM_LBUTTONUP => overlay::open(),
            WM_RBUTTONUP => match tray::menu(window, &settings().hotkey, update::menu_item().as_deref()) {
                Some(tray::Command::Capture) => overlay::open(),
                Some(tray::Command::Settings) => preferences::open(),
                Some(tray::Command::Guide) => guide::open(),
                Some(tray::Command::Update) => update::menu_chosen(),
                Some(tray::Command::Support) => open_site(site::SUPPORT),
                Some(tray::Command::Exit) => unsafe {
                    DestroyWindow(window);
                },
                None => {}
            },
            NIN_BALLOONUSERCLICK => {
                if let Some(path) = SAVED.with_borrow(Clone::clone)
                    && let Err(problem) = shell::show_in_folder(&path)
                {
                    notify(tr("The folder didn't open"), &problem);
                }
            }
            _ => {}
        },
        WM_DESTROY => {
            TRAY.with(|t| t.borrow_mut().take());
            preferences::close();
            guide::close();
            languages::close();
            keys::close();
            invite::close();
            browser::close();
            shell::remove_pages(Duration::ZERO);
            hotkey::release(window);
            unsafe { PostQuitMessage(0) };
        }
        _ if TASKBAR_CREATED.get() == Some(&message) => TRAY.with(|t| {
            if let Some(tray) = &*t.borrow() {
                let _ = tray.show();
            }
        }),
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}
