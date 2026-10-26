//! Everything that talks to Windows: the hotkey, screen capture, the clipboard, the browser,
//! the icon in the notification area and the windows themselves, the program's own browser
//! window among them. The selection, the
//! annotations, the queries and how things look don't know about Windows, so that these parts
//! could be replaced (decision 2).

pub mod app;
mod browser;
mod capture;
mod card;
mod clipboard;
mod gdi;
mod guide;
mod hotkey;
mod http;
mod invite;
mod keys;
mod languages;
mod masking;
mod models;
mod ocr;
mod overlay;
mod preferences;
mod shell;
mod tray;
mod update;

use std::io;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_XVIRTUALSCREEN};

/// Whether the program's windows open out of sight. The tests look at them through the browser's
/// developer protocol and in pictures of their own, so there the windows open off the screens,
/// without taking the focus or a button on the taskbar, and the user can go on working meanwhile.
const QUIET: bool = cfg!(test);

/// Where a window opens out of sight: just right of all the screens, at the size it has.
fn out_of_sight(place: RECT) -> RECT {
    let left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN) } + 64;
    RECT { left, top: place.top, right: left + place.right - place.left, bottom: place.bottom }
}

/// A string for Windows: UTF-16 with a zero at the end.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// The error of the last Windows call that failed.
fn last_error() -> io::Error {
    io::Error::last_os_error()
}
