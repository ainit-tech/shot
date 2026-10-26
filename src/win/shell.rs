//! The rest of Windows that the program needs: opening addresses and files, the user's folders,
//! the save dialog, the clock, the user's language and starting together with Windows.

use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{Duration, SystemTime};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, HWND, SYSTEMTIME};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegCloseKey, RegDeleteValueW,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Shell::{
    ASSOCF_NONE, ASSOCSTR_COMMAND, AssocQueryStringW, FOLDERID_Pictures, FOLDERID_Screenshots, KF_FLAG_CREATE,
    SHGetKnownFolderPath, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, SW_SHOWNORMAL};
use windows_sys::core::{GUID, PCWSTR, PWSTR};
use windows_sys::w;

use super::wide;
use crate::text::{Language, tr};

/// The language of Windows as the program speaks it: the first of the user's languages, or
/// English for one the program doesn't speak.
pub fn windows_language() -> Language {
    use windows::System::UserProfile::GlobalizationPreferences;
    let first = GlobalizationPreferences::Languages().ok().and_then(|tags| tags.into_iter().next());
    first.map_or(Language::EN, |tag| Language::from_tag(&tag.to_string()))
}

/// Where the installer leaves the language it spoke.
const PROGRAM: PCWSTR = w!("Software\\ainitShot");

/// The language of an installation since the last start (decision 11): the installer leaves it,
/// and the program takes it away as it reads it, so that it counts once. Nothing if no
/// installation has come since, or if it names no language the program speaks.
pub fn installer_language() -> Option<Language> {
    let mut code = [0u16; 16];
    let mut size = size_of_val(&code) as u32;
    let found = unsafe {
        let value = w!("Language");
        RegGetValueW(HKEY_CURRENT_USER, PROGRAM, value, RRF_RT_REG_SZ, null_mut(), code.as_mut_ptr().cast(), &mut size)
    } == 0;
    let mut key: HKEY = null_mut();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PROGRAM, 0, KEY_SET_VALUE, &mut key) } == 0 {
        unsafe {
            RegDeleteValueW(key, w!("Language"));
            RegCloseKey(key);
        }
    }
    let len = code.iter().position(|&c| c == 0).unwrap_or(code.len());
    found.then(|| Language::from_code(&String::from_utf16_lossy(&code[..len]))).flatten()
}

/// Opens an address in the user's browser, or a file in its program.
pub fn open(target: &str) -> Result<(), String> {
    // The browser may come to the front only if the program, now in front, lets it.
    unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    let result = unsafe { ShellExecuteW(null_mut(), w!("open"), wide(target).as_ptr(), null(), null(), SW_SHOWNORMAL) };
    if result as isize > 32 { Ok(()) } else { Err(cannot_open(target)) }
}

fn cannot_open(target: &str) -> String {
    tr("Windows couldn't open {}.").replace("{}", target)
}

/// Writes a page into the program's temporary folder and opens it in the browser. The pages of
/// earlier queries are deleted first: they hold screenshots, and the browser has long read them.
pub fn open_page(html: &str) -> Result<(), String> {
    let folder = pages_folder();
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    remove_pages(Duration::from_secs(60));
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let path = folder.join(format!("query-{stamp}.html"));
    fs::write(&path, html).map_err(|e| e.to_string())?;
    open_in_browser(&path)
}

/// Deletes the pages of queries that are older than `age`.
pub fn remove_pages(age: Duration) {
    let Ok(entries) = fs::read_dir(pages_folder()) else { return };
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e >= age);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn pages_folder() -> PathBuf {
    std::env::temp_dir().join("ainitShot")
}

/// Opens a local file in the browser that opens web addresses, which is not always the program
/// set for its kind of file.
fn open_in_browser(path: &Path) -> Result<(), String> {
    let url = file_url(path);
    if let Some(command) = browser_command() {
        let (program, arguments) = split_command(&command);
        let arguments = if arguments.contains("%1") { arguments.replace("%1", &url) } else { format!("{arguments} {url}") };
        unsafe { AllowSetForegroundWindow(ASFW_ANY) };
        let (program, arguments) = (wide(program), wide(&arguments));
        let result =
            unsafe { ShellExecuteW(null_mut(), w!("open"), program.as_ptr(), arguments.as_ptr(), null(), SW_SHOWNORMAL) };
        if result as isize > 32 {
            return Ok(());
        }
    }
    open(&path.to_string_lossy())
}

/// How Windows starts the default browser for a web address, such as
/// `"C:\…\opera.exe" --single-argument %1`.
fn browser_command() -> Option<String> {
    let mut buffer = vec![0u16; 2048];
    let mut len = buffer.len() as u32;
    let result =
        unsafe { AssocQueryStringW(ASSOCF_NONE, ASSOCSTR_COMMAND, w!("https"), w!("open"), buffer.as_mut_ptr(), &mut len) };
    (result >= 0).then(|| String::from_utf16_lossy(&buffer[..(len as usize).saturating_sub(1)]))
}

/// The program and the rest of a command line; the program may be in quotes.
fn split_command(command: &str) -> (&str, &str) {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"')
        && let Some(end) = rest.find('"')
    {
        return (&rest[..end], rest[end + 1..].trim());
    }
    command.split_once(' ').map_or((command, ""), |(program, arguments)| (program, arguments.trim()))
}

/// A `file:///` address for a path, without spaces or other characters a command line would split.
fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let parts: Vec<String> =
        text.split('/').enumerate().map(|(i, part)| if i == 0 { part.to_string() } else { crate::query::encode(part) }).collect();
    format!("file:///{}", parts.join("/"))
}

/// Opens a file's folder in Explorer with the file selected, in a window already open there, if
/// there is one.
pub fn show_in_folder(path: &Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};
    use windows_core::PCWSTR as ComText;
    let text = path.to_string_lossy();
    let wide_path = wide(&text);
    unsafe {
        let item = ILCreateFromPathW(ComText(wide_path.as_ptr()));
        if item.is_null() {
            return Err(cannot_open(&text));
        }
        // Explorer may come to the front only if the program lets it.
        AllowSetForegroundWindow(ASFW_ANY);
        let shown = SHOpenFolderAndSelectItems(item, None, 0);
        ILFree(Some(item));
        shown.map_err(|_| cannot_open(&text))
    }
}

/// Opens a text file in its program, or in Notepad if no program is set for its kind.
pub fn open_text(path: &Path) -> Result<(), String> {
    let path = path.to_string_lossy();
    if open(&path).is_ok() {
        return Ok(());
    }
    let quoted = wide(&format!("\"{path}\""));
    let result = unsafe { ShellExecuteW(null_mut(), w!("open"), w!("notepad.exe"), quoted.as_ptr(), null(), SW_SHOWNORMAL) };
    if result as isize > 32 { Ok(()) } else { Err(cannot_open(&path)) }
}

/// Where the settings file is: `%APPDATA%\ainitShot\settings.toml`.
pub fn settings_path() -> PathBuf {
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    roaming.join("ainitShot").join("settings.toml")
}

/// The profile of the program's browser window, where the sign-in to Google is kept:
/// `%LOCALAPPDATA%\ainitShot\browser`.
pub fn browser_profile() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    local.join("ainitShot").join("browser")
}

/// The folder for screenshots: "Pictures\Screenshots", or "Pictures" if there is none.
pub fn screenshots_folder() -> PathBuf {
    known_folder(&FOLDERID_Screenshots)
        .or_else(|| known_folder(&FOLDERID_Pictures))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut path: PWSTR = null_mut();
    let result = unsafe { SHGetKnownFolderPath(id, KF_FLAG_CREATE as u32, null_mut(), &mut path) };
    let folder = (result >= 0 && !path.is_null()).then(|| {
        let len = (0..).take_while(|&i| unsafe { *path.add(i) } != 0).count();
        PathBuf::from(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(path, len) }))
    });
    unsafe { CoTaskMemFree(path.cast()) };
    folder
}

/// The name of a new screenshot from the pattern in the settings, with `{date}` and `{time}`
/// filled in and the characters a file name can't have replaced.
pub fn file_name(pattern: &str) -> String {
    let mut time = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut time) };
    let date = format!("{:04}-{:02}-{:02}", time.wYear, time.wMonth, time.wDay);
    let clock = format!("{:02}-{:02}-{:02}", time.wHour, time.wMinute, time.wSecond);
    let name = pattern.replace("{date}", &date).replace("{time}", &clock);
    name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '-' } else { c }).collect::<String>().trim().to_string()
}

/// A path in `folder` for a file named `name` with the extension `.png` that doesn't exist yet:
/// "name (2).png" and so on if it does.
pub fn new_path(folder: &Path, name: &str) -> PathBuf {
    let mut path = folder.join(format!("{name}.png"));
    let mut number = 2;
    while path.exists() {
        path = folder.join(format!("{name} ({number}).png"));
        number += 1;
    }
    path
}

/// Asks where to save a PNG file, starting in `folder` with the name `name`. Nothing if the
/// user cancels.
pub fn ask_where_to_save(owner: HWND, folder: &Path, name: &str) -> Option<PathBuf> {
    let mut file = vec![0u16; 1024];
    let initial: Vec<u16> = name.encode_utf16().take(file.len() - 1).collect();
    file[..initial.len()].copy_from_slice(&initial);
    let folder = wide(&folder.to_string_lossy());
    let filter: Vec<u16> = "PNG\0*.png\0\0".encode_utf16().collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrInitialDir: folder.as_ptr(),
        lpstrDefExt: w!("png"),
        Flags: OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if unsafe { GetSaveFileNameW(&mut dialog) } == 0 {
        return None;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..len])))
}

/// Asks for a folder with the dialog of Windows, starting in `folder`. Nothing if the user
/// cancels.
pub fn pick_folder(owner: HWND, folder: &Path) -> Option<PathBuf> {
    use windows::Win32::Foundation::HWND as ComWindow;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree as ComFree};
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
        SIGDN_FILESYSPATH,
    };
    use windows_core::{HSTRING, PCWSTR as ComText};
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        let title = wide(tr("Folder for screenshots"));
        let _ = dialog.SetTitle(ComText(title.as_ptr()));
        if let Ok(start) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(folder.as_os_str()), None) {
            let _ = dialog.SetFolder(&start);
        }
        // Cancelling the dialog comes back as an error.
        dialog.Show(Some(ComWindow(owner))).ok()?;
        let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok().map(PathBuf::from);
        ComFree(Some(name.0.cast()));
        path
    }
}

const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
/// The list of programs that start with Windows, in Task Manager and in Settings: a program of Run
/// turned off there has a record whose first byte is odd.
const STARTUP_APPROVED: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");

/// Whether Windows starts the program as it starts: it is in Run, and the list of programs that
/// start with Windows hasn't turned it off. The installer reads it so too.
pub fn starts_with_windows() -> bool {
    let listed =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN, w!("ainitShot"), RRF_RT_REG_SZ, null_mut(), null_mut(), null_mut()) } == 0;
    let mut record = [0u8; 64];
    let mut size = record.len() as u32;
    let found = unsafe {
        let value = w!("ainitShot");
        RegGetValueW(HKEY_CURRENT_USER, STARTUP_APPROVED, value, RRF_RT_REG_BINARY, null_mut(), record.as_mut_ptr().cast(), &mut size)
    } == 0;
    listed && !(found && size > 0 && record[0] & 1 == 1)
}

/// Starts the program together with Windows, or stops doing so (decision 3). Its record in the
/// list of programs that start with Windows goes either way, so that the list doesn't keep it
/// off; the installer does the same.
pub fn set_autostart(on: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut approved: HKEY = null_mut();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, STARTUP_APPROVED, 0, KEY_SET_VALUE, &mut approved) } == 0 {
        unsafe {
            RegDeleteValueW(approved, w!("ainitShot"));
            RegCloseKey(approved);
        }
    }
    let mut key: HKEY = null_mut();
    let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN, 0, KEY_SET_VALUE, &mut key) };
    if opened != 0 {
        return Err(std::io::Error::from_raw_os_error(opened as i32).to_string());
    }
    let result = if on {
        let value = wide(&format!("\"{}\"", exe.display()));
        unsafe { RegSetValueExW(key, w!("ainitShot"), 0, REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32) }
    } else {
        match unsafe { RegDeleteValueW(key, w!("ainitShot")) } {
            ERROR_FILE_NOT_FOUND => 0,
            other => other,
        }
    };
    unsafe { RegCloseKey(key) };
    if result == 0 { Ok(()) } else { Err(std::io::Error::from_raw_os_error(result as i32).to_string()) }
}
