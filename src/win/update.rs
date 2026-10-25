//! Updates on Windows (decision 13). A thread of its own fetches the description of the latest
//! release with WinHTTP and checks its signature with the program's public key; the installer of
//! a newer version is downloaded the same way and checked against the size and SHA-256 the
//! description gives. The program runs the installer silently once none of its windows is open
//! and closes; the installer replaces it and starts the new version. Whatever goes wrong waits
//! for the next check, and a program run from its source never updates itself.

use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use windows_sys::Win32::Foundation::{HWND, LPARAM, SYSTEMTIME};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, IsWindowVisible, PostMessageW, SW_HIDE, SetTimer, WM_APP, WM_CLOSE,
};
use windows_sys::core::BOOL;
use windows_sys::w;

use super::http::{self, Failure, Get};
use super::{app, preferences, wide};
use crate::text::tr;
use crate::update::{self, Hash, Release, Version};

/// The public key the description of every release is signed with, in minisign's form, key
/// 54DD8F86E10873AE. Its secret half exists only in the CI secrets of the repository.
const PUBLIC_KEY: &str = "RWSucwjhho/dVEb5v8k1R5yDn1599t66tZaLo2TVUwKArY3XVwKnw8EW";

/// The message a check sends to the program's window as it goes.
pub const MESSAGE: u32 = WM_APP + 10;
/// The timer of the checks, on the program's window.
pub const TIMER: usize = 0x5550;
/// The first check comes a little after the start, so as not to slow it.
const FIRST: u32 = 30_000;
/// How often the timer looks whether a day has passed, or whether the program's windows are
/// closed for an update to be installed.
const TICK: u32 = 60_000;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
/// Limits on what the site may send: the description and its signature are small.
const DESCRIPTION: u64 = 64 * 1024;
const SIGNATURE: u64 = 4 * 1024;
/// How long a check may take at most, and the download of a new version.
const CHECKING: Duration = Duration::from_secs(60);
const DOWNLOADING: Duration = Duration::from_secs(15 * 60);

/// Where the program stands with updates.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Built and run from its source: such a program never updates itself.
    Source,
    /// Not checked yet.
    Waiting,
    Checking,
    Latest,
    /// A newer version, not downloaded: the settings say not to install by itself.
    Available(Version),
    Downloading(Version),
    /// A newer version downloaded and checked, waiting for the program's windows to close.
    Ready(Version),
    Failed(Problem),
}

/// What kept a check or an update from succeeding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Offline,
    Server,
    Damaged,
    Signature,
    File,
    Disk,
    Installer,
}

impl Problem {
    pub fn text(self) -> &'static str {
        match self {
            Problem::Offline => tr("No connection to the program's site."),
            Problem::Server => tr("The program's site answered with an error."),
            Problem::Damaged => tr("The description of the new version is damaged."),
            Problem::Signature => tr("The description of the new version isn't signed with the program's key."),
            Problem::File => tr("The downloaded file doesn't match the description of the new version."),
            Problem::Disk => tr("The new version couldn't be saved."),
            Problem::Installer => tr("The installer of the new version didn't start."),
        }
    }
}

struct Updates {
    window: HWND,
    status: Status,
    /// When the last check ended, whatever came of it.
    checked: Option<Instant>,
    /// The time of day it ended, for the settings window.
    checked_at: Option<String>,
    working: bool,
    /// The user asked to install the new version: it is installed at once, whatever the settings.
    install: bool,
    /// The user asked from the tray menu, so the outcome comes as a notification.
    asked: bool,
    installer: Option<PathBuf>,
}

thread_local! {
    static UPDATES: RefCell<Option<Updates>> = const { RefCell::new(None) };
}

/// What the thread of a check reports to the program's window.
enum Report {
    Downloading(Version),
    Done(Result<Found, Problem>),
}

enum Found {
    Latest,
    Newer(Version),
    Downloaded(Version, PathBuf),
}

/// Starts the checks: the first a little after the start, then once a day.
pub fn start(window: HWND) {
    let installed = installed();
    let status = if installed { Status::Waiting } else { Status::Source };
    let updates = Updates {
        window,
        status,
        checked: None,
        checked_at: None,
        working: false,
        install: false,
        asked: false,
        installer: None,
    };
    UPDATES.with(|cell| *cell.borrow_mut() = Some(updates));
    if installed {
        clean();
        unsafe { SetTimer(window, TIMER, FIRST, None) };
    }
}

pub fn status() -> Status {
    UPDATES.with(|cell| cell.borrow().as_ref().map_or(Status::Source, |u| u.status.clone()))
}

/// The item of updates in the tray menu: "Update to …" once a newer version is known, "Check for
/// updates" otherwise, and none in a program run from its source.
pub fn menu_item() -> Option<String> {
    match status() {
        Status::Source => None,
        Status::Available(version) | Status::Downloading(version) | Status::Ready(version) => {
            Some(tr("Update to {version}").replace("{version}", &version.to_string()))
        }
        _ => Some(tr("Check for updates").into()),
    }
}

/// The item of updates was chosen in the tray menu.
pub fn menu_chosen() {
    match status() {
        Status::Available(_) | Status::Ready(_) => install_now(),
        Status::Checking | Status::Downloading(_) => {}
        _ => check_now(),
    }
}

/// The timer: a check once a day, and the installation of a downloaded version once none of the
/// program's windows is open.
pub fn tick() {
    let Some((window, due, ready)) = UPDATES.with(|cell| {
        let cell = cell.borrow();
        let u = cell.as_ref()?;
        let due = !u.working && u.checked.is_none_or(|checked| checked.elapsed() >= DAY);
        Some((u.window, due, matches!(u.status, Status::Ready(_))))
    }) else {
        return;
    };
    unsafe { SetTimer(window, TIMER, TICK, None) };
    let automatic = app::settings().autoupdate;
    if ready && automatic && idle() {
        install();
    } else if due {
        check(false, automatic);
    }
}

/// "Check for updates": a newer version is installed at once if the settings say so.
pub fn check_now() {
    check(true, app::settings().autoupdate);
}

/// "Update": installs the newer version, downloading it first if it isn't yet.
pub fn install_now() {
    let ready = UPDATES.with(|cell| {
        let mut cell = cell.borrow_mut();
        let Some(u) = cell.as_mut() else { return false };
        u.install = true;
        u.asked = true;
        matches!(u.status, Status::Ready(_))
    });
    if ready { install() } else { check(true, true) }
}

fn check(asked: bool, download: bool) {
    let Some(window) = UPDATES.with(|cell| {
        let mut cell = cell.borrow_mut();
        let u = cell.as_mut()?;
        if u.working || u.status == Status::Source {
            return None;
        }
        u.working = true;
        u.asked |= asked;
        u.status = Status::Checking;
        Some(u.window)
    }) else {
        return;
    };
    changed();
    let target = window as isize;
    thread::spawn(move || {
        let post = |report: Report| {
            let report = Box::into_raw(Box::new(report));
            if unsafe { PostMessageW(target as HWND, MESSAGE, 0, report as isize) } == 0 {
                drop(unsafe { Box::from_raw(report) });
            }
        };
        let found = fetch(download, &post);
        post(Report::Done(found));
    });
}

/// What a check found, on the program's window.
pub fn heard(report: isize) {
    let report = *unsafe { Box::from_raw(report as *mut Report) };
    let found = match report {
        Report::Downloading(version) => {
            set_status(Status::Downloading(version));
            changed();
            return;
        }
        Report::Done(found) => found,
    };
    let Some((asked, asked_to_install)) = UPDATES.with(|cell| {
        let mut cell = cell.borrow_mut();
        let u = cell.as_mut()?;
        u.working = false;
        u.checked = Some(Instant::now());
        u.checked_at = Some(now());
        let asked = std::mem::take(&mut u.asked);
        match &found {
            Ok(Found::Latest) => u.status = Status::Latest,
            Ok(Found::Newer(version)) => u.status = Status::Available(*version),
            Ok(Found::Downloaded(version, path)) => {
                u.status = Status::Ready(*version);
                u.installer = Some(path.clone());
            }
            Err(problem) => u.status = Status::Failed(*problem),
        }
        Some((asked, u.install))
    }) else {
        return;
    };
    let current = Version::current().to_string();
    match found {
        Ok(Found::Latest) => {
            log(&format!("{current} is the latest"));
            if asked {
                app::announce(tr("No updates"), &tr("Version {version} is the latest.").replace("{version}", &current));
            }
        }
        Ok(Found::Newer(version)) => {
            log(&format!("{version} is available"));
            if asked {
                let title = tr("ainitShot {version} is available").replace("{version}", &version.to_string());
                app::announce(&title, tr("It is installed from the icon's menu or from the settings."));
            }
        }
        Ok(Found::Downloaded(version, _)) => {
            log(&format!("{version} is downloaded"));
            // Asked for, the update comes at once; by itself, once the program's windows close,
            // which the timer looks for.
            if asked_to_install || (app::settings().autoupdate && idle()) {
                install();
                return;
            }
        }
        Err(problem) => {
            log(&format!("{problem:?}: {}", problem.text()));
            if asked {
                app::notify(tr("Couldn't update"), problem.text());
            }
        }
    }
    changed();
}

/// Runs the installer silently and closes the program as "Exit" does; the installer waits for
/// it to close, replaces it and starts the new version.
fn install() {
    let Some((window, installer)) = UPDATES.with(|cell| cell.borrow().as_ref().and_then(|u| Some((u.window, u.installer.clone()?))))
    else {
        return;
    };
    let file = wide(&installer.to_string_lossy());
    let started = unsafe { ShellExecuteW(null_mut(), w!("open"), file.as_ptr(), w!("/S /UPDATE"), null(), SW_HIDE) } as isize > 32;
    if started {
        log(&format!("installing from {}", installer.display()));
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
    } else {
        log("the installer didn't start");
        set_status(Status::Failed(Problem::Installer));
        changed();
    }
}

fn set_status(status: Status) {
    UPDATES.with(|cell| {
        if let Some(u) = cell.borrow_mut().as_mut() {
            u.status = status;
        }
    });
}

/// The settings window shows the new state.
fn changed() {
    preferences::updates(&describe());
}

/// The state for the settings window.
pub fn describe() -> Value {
    let (status, checked) = UPDATES.with(|cell| {
        cell.borrow().as_ref().map_or((Status::Source, None), |u| (u.status.clone(), u.checked_at.clone()))
    });
    let (state, version, problem) = match status {
        Status::Source => ("source", None, None),
        Status::Waiting => ("waiting", None, None),
        Status::Checking => ("checking", None, None),
        Status::Latest => ("latest", None, None),
        Status::Available(version) => ("available", Some(version), None),
        Status::Downloading(version) => ("downloading", Some(version), None),
        Status::Ready(version) => ("ready", Some(version), None),
        Status::Failed(problem) => ("failed", None, Some(problem.text())),
    };
    json!({
        "current": Version::current().to_string(),
        "state": state,
        "version": version.map(|v| v.to_string()),
        "problem": problem,
        "checked": checked,
    })
}

/// Whether the program runs from where its installer put it.
fn installed() -> bool {
    let (Ok(exe), Some(local)) = (std::env::current_exe(), std::env::var_os("LOCALAPPDATA")) else { return false };
    let home = PathBuf::from(local).join("Programs").join("ainitShot");
    exe.parent().is_some_and(|folder| folder.to_string_lossy().eq_ignore_ascii_case(&home.to_string_lossy()))
}

/// Whether none of the program's windows is open: no frozen screen, settings, conversation or
/// message.
fn idle() -> bool {
    unsafe extern "system" fn visible(window: HWND, found: LPARAM) -> BOOL {
        if unsafe { IsWindowVisible(window) } == 0 {
            return 1;
        }
        unsafe { *(found as *mut bool) = true };
        0
    }
    let mut found = false;
    unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(visible), &mut found as *mut bool as LPARAM) };
    !found
}

/// The folder of downloaded versions and of the log: `%LOCALAPPDATA%\ainitShot\updates`.
fn folder() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    local.join("ainitShot").join("updates")
}

/// Removes the installers of earlier updates; the log stays, cut once it grows.
fn clean() {
    let Ok(entries) = fs::read_dir(folder()) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "exe" || e == "part") {
            let _ = fs::remove_file(path);
        }
    }
    let log = folder().join("log.txt");
    if fs::metadata(&log).is_ok_and(|m| m.len() > 64 * 1024) {
        let _ = fs::rename(&log, folder().join("log.old.txt"));
    }
}

/// Adds a line to the log of updates.
fn log(line: &str) {
    let _ = fs::create_dir_all(folder());
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(folder().join("log.txt")) {
        let _ = writeln!(file, "{} {line}", now());
    }
}

/// The date and time of day, as the log and the settings window show it.
fn now() -> String {
    let mut time = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut time) };
    format!("{:04}-{:02}-{:02} {:02}:{:02}", time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute)
}

/// The check itself, on a thread of its own: the description, its signature, and the installer
/// of a newer version if `download`.
fn fetch(download: bool, post: &dyn Fn(Report)) -> Result<Found, Problem> {
    let deadline = Instant::now() + CHECKING;
    let description = get(update::MANIFEST, DESCRIPTION, deadline)?;
    let signature = get(&format!("{}.minisig", update::MANIFEST), SIGNATURE, deadline)?;
    let signature = String::from_utf8(signature).map_err(|_| Problem::Signature)?;
    if !update::signed(&description, &signature, PUBLIC_KEY) {
        return Err(Problem::Signature);
    }
    let Some(release) = update::offer(&description, Version::current()).map_err(|_| Problem::Damaged)? else {
        return Ok(Found::Latest);
    };
    if !download {
        return Ok(Found::Newer(release.version));
    }
    post(Report::Downloading(release.version));
    let path = download_installer(&release)?;
    Ok(Found::Downloaded(release.version, path))
}

/// Downloads the installer of `release` into the folder of updates and checks it; an installer
/// downloaded and checked before is kept.
fn download_installer(release: &Release) -> Result<PathBuf, Problem> {
    let folder = folder();
    fs::create_dir_all(&folder).map_err(|_| Problem::Disk)?;
    let path = folder.join(format!("ainitShot-{}-setup.exe", release.version));
    if fs::read(&path).is_ok_and(|bytes| fits(&bytes, release)) {
        return Ok(path);
    }
    let part = path.with_extension("part");
    let mut file = fs::File::create(&part).map_err(|_| Problem::Disk)?;
    let (mut hash, mut size) = (Hash::default(), 0u64);
    let deadline = Some(Instant::now() + DOWNLOADING);
    let got = http::get_into(&Get::whole(&release.file.url, release.file.size, deadline), &mut |bytes| {
        hash.update(bytes);
        size += bytes.len() as u64;
        file.write_all(bytes).map_err(|_| Problem::Disk)
    })
    .map_err(problem);
    drop(file);
    let checked = got.and_then(|()| {
        if size == release.file.size && hash.hex() == release.file.sha256 { Ok(()) } else { Err(Problem::File) }
    });
    match checked.and_then(|()| fs::rename(&part, &path).map_err(|_| Problem::Disk)) {
        Ok(()) => Ok(path),
        Err(problem) => {
            let _ = fs::remove_file(&part);
            Err(problem)
        }
    }
}

/// Whether `bytes` are the file the release describes.
fn fits(bytes: &[u8], release: &Release) -> bool {
    let mut hash = Hash::default();
    hash.update(bytes);
    bytes.len() as u64 == release.file.size && hash.hex() == release.file.sha256
}

fn get(url: &str, limit: u64, deadline: Instant) -> Result<Vec<u8>, Problem> {
    let mut body = Vec::new();
    http::get_into(&Get::whole(url, limit, Some(deadline)), &mut |bytes| {
        body.extend_from_slice(bytes);
        Ok(())
    })
    .map_err(problem)?;
    Ok(body)
}

/// The problem of a check when fetching failed.
fn problem(failure: Failure<Problem>) -> Problem {
    match failure {
        Failure::Offline => Problem::Offline,
        Failure::Server => Problem::Server,
        Failure::Stopped(problem) => problem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_run_from_its_source_doesnt_count_as_installed() {
        assert!(!installed());
    }

    #[test]
    fn the_programs_key_is_a_key() {
        assert!(minisign_verify::PublicKey::from_base64(PUBLIC_KEY).is_ok());
    }

    #[test]
    fn a_file_is_kept_only_if_it_matches_its_description() {
        let release = Release {
            version: Version::parse("1.0.0").unwrap(),
            file: update::File {
                url: "https://shot.ainit.tech/x".into(),
                size: 4,
                sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".into(),
            },
        };
        assert!(fits(b"test", &release));
        assert!(!fits(b"Test", &release));
        assert!(!fits(b"test!", &release));
    }
}
