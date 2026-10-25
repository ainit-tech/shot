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

/// A string for Windows: UTF-16 with a zero at the end.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// The error of the last Windows call that failed.
fn last_error() -> io::Error {
    io::Error::last_os_error()
}
