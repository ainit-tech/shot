//! The clipboard: putting a picture into it as a device-independent bitmap, the standard format
//! that every program pastes, and text, and taking text out of it.

use std::io;
use std::ptr::{self, null_mut};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};

use super::gdi::bitmap_info;
use super::last_error;
use crate::image::Image;

pub fn put(owner: HWND, image: &Image) -> io::Result<()> {
    let header = BITMAPINFOHEADER {
        biSizeImage: image.pixels.len() as u32,
        // A positive height: in this format rows go from the bottom, as most programs expect.
        ..bitmap_info(image.width as i32, image.height as i32).bmiHeader
    };
    let header = unsafe {
        std::slice::from_raw_parts((&header as *const BITMAPINFOHEADER).cast::<u8>(), size_of::<BITMAPINFOHEADER>())
    };
    let mut bitmap = Vec::with_capacity(header.len() + image.pixels.len());
    bitmap.extend_from_slice(header);
    for row in image.pixels.chunks_exact(image.stride()).rev() {
        bitmap.extend_from_slice(row);
    }

    if !open(owner) {
        return Err(last_error());
    }
    unsafe { EmptyClipboard() };
    let placed = place(CF_DIB as u32, &bitmap);
    unsafe { CloseClipboard() };
    placed
}

/// Puts text into the clipboard.
pub fn put_text(owner: HWND, text: &str) -> io::Result<()> {
    let wide: Vec<u16> = text.replace('\n', "\r\n").encode_utf16().chain(Some(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
    if !open(owner) {
        return Err(last_error());
    }
    unsafe { EmptyClipboard() };
    let placed = place(CF_UNICODETEXT as u32, bytes);
    unsafe { CloseClipboard() };
    placed
}

/// The text in the clipboard, if it has any.
pub fn text(owner: HWND) -> Option<String> {
    if !open(owner) {
        return None;
    }
    let text = unsafe {
        let memory = GetClipboardData(CF_UNICODETEXT as u32);
        let start = if memory.is_null() { null_mut() } else { GlobalLock(memory).cast::<u16>() };
        if start.is_null() {
            None
        } else {
            let most = GlobalSize(memory) / 2;
            let len = (0..most).take_while(|&i| *start.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(start, len));
            GlobalUnlock(memory);
            Some(text)
        }
    };
    unsafe { CloseClipboard() };
    text
}

/// Hands the clipboard a copy of the data in the given format. The clipboard owns that memory
/// from then on, unless it refuses to take it.
fn place(format: u32, data: &[u8]) -> io::Result<()> {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, data.len());
        if memory.is_null() {
            return Err(last_error());
        }
        let target = GlobalLock(memory).cast::<u8>();
        if target.is_null() {
            let error = last_error();
            GlobalFree(memory);
            return Err(error);
        }
        ptr::copy_nonoverlapping(data.as_ptr(), target, data.len());
        GlobalUnlock(memory);
        if SetClipboardData(format, memory).is_null() {
            let error = last_error();
            GlobalFree(memory);
            return Err(error);
        }
    }
    Ok(())
}

/// Opens the clipboard. Another program may be holding it for a moment, so it is tried a few
/// times.
fn open(owner: HWND) -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(owner) } != 0 {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}
