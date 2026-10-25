//! The icon in the notification area. A click on it takes a screenshot; its menu also opens the
//! settings and the presentation of how to use the program, and closes the program. It is also
//! where the program tells the user about problems.

use std::io;
use std::ptr::{self, null, null_mut};

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject};
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, HICON, ICON_BIG,
    ICON_SMALL, ICONINFO, MF_SEPARATOR, MF_STRING, PostMessageW, SM_CXICON, SM_CXSMICON, SendMessageW,
    SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_NULL, WM_SETICON,
};
use super::gdi::bitmap_info;
use super::{last_error, wide};
use crate::draw;
use crate::text::tr;

/// The message the icon sends to the program's window.
pub const MESSAGE: u32 = WM_APP + 1;
const ID: u32 = 1;

pub enum Command {
    Capture,
    Settings,
    Guide,
    Exit,
}

const CAPTURE: usize = 1;
const SETTINGS: usize = 2;
const EXIT: usize = 3;
const GUIDE: usize = 4;

pub struct Tray {
    window: HWND,
    icon: HICON,
}

impl Tray {
    pub fn add(window: HWND) -> io::Result<Tray> {
        let tray = Tray { window, icon: create_icon()? };
        tray.show()?;
        Ok(tray)
    }

    /// Puts the icon into the notification area; again after Explorer restarts.
    pub fn show(&self) -> io::Result<()> {
        let mut data = self.data();
        data.uFlags |= NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = MESSAGE;
        data.hIcon = self.icon;
        copy_into(&mut data.szTip, "ainitShot");
        if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
            return Err(io::Error::other(tr("Couldn't add the icon to the notification area.")));
        }
        Ok(())
    }

    /// Shows a notification next to the icon.
    pub fn notify(&self, title: &str, text: &str) {
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_WARNING;
        copy_into(&mut data.szInfoTitle, title);
        copy_into(&mut data.szInfo, text);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: self.window, uID: ID, ..Default::default() }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.data());
            DestroyIcon(self.icon);
        }
    }
}

/// Copies text into a fixed buffer of a Windows structure, cutting it if it is too long.
fn copy_into(buffer: &mut [u16], text: &str) {
    let text: Vec<u16> = text.encode_utf16().take(buffer.len() - 1).collect();
    buffer[..text.len()].copy_from_slice(&text);
    buffer[text.len()] = 0;
}

/// Shows the icon's menu at the mouse and returns what was chosen. `hotkey` is shown next to
/// the screenshot item. Google's account isn't here: one signs in once, from the offer as the
/// program starts or from the settings window.
pub fn menu(window: HWND, hotkey: &str) -> Option<Command> {
    let capture = wide(&format!("{}\t{hotkey}", tr("Take a screenshot")));
    let settings = wide(tr("Settings"));
    let guide = wide(tr("How to use"));
    let exit = wide(tr("Exit"));
    unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, CAPTURE, capture.as_ptr());
        AppendMenuW(menu, MF_STRING, SETTINGS, settings.as_ptr());
        AppendMenuW(menu, MF_STRING, GUIDE, guide.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, EXIT, exit.as_ptr());
        let mut at = POINT::default();
        GetCursorPos(&mut at);
        // Without this the menu doesn't close when the user clicks elsewhere.
        SetForegroundWindow(window);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, 0, window, null());
        PostMessageW(window, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match chosen as usize {
            CAPTURE => Some(Command::Capture),
            SETTINGS => Some(Command::Settings),
            GUIDE => Some(Command::Guide),
            EXIT => Some(Command::Exit),
            _ => None,
        }
    }
}

/// The program's icon at the size the notification area uses on this computer.
fn create_icon() -> io::Result<HICON> {
    create_icon_of(unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) })
}

/// Gives a window of the program the program's icon, on its title bar and on the taskbar, at the
/// window's scale. The icons are the caller's to destroy once the window is gone.
pub fn give_icon(window: HWND) -> Vec<HICON> {
    let dpi = unsafe { GetDpiForWindow(window) };
    let mut icons = Vec::new();
    for (kind, size) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
        if let Ok(icon) = create_icon_of(unsafe { GetSystemMetricsForDpi(size, dpi) }) {
            unsafe { SendMessageW(window, WM_SETICON, kind as usize, icon as isize) };
            icons.push(icon);
        }
    }
    icons
}

/// The program's icon, `size` pixels square.
fn create_icon_of(size: i32) -> io::Result<HICON> {
    let size = size.max(16);
    let pixmap = draw::app_icon(size as u32)
        .ok_or_else(|| io::Error::other(tr("Couldn't draw the icon.")))?;
    // An icon keeps its colours apart from their opacity.
    let pixels: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    unsafe {
        let mut bits = null_mut();
        let color = CreateDIBSection(null_mut(), &bitmap_info(size, -size), DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if color.is_null() {
            return Err(last_error());
        }
        ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
        // Windows ignores the mask of an icon with opacity, but an icon must have one. Its
        // rows are padded to 16 bits.
        let mask_bits = vec![0u8; (size as usize).div_ceil(16) * 2 * size as usize];
        let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr().cast());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        let error = icon.is_null().then(last_error);
        DeleteObject(color);
        DeleteObject(mask);
        match error {
            Some(error) => Err(error),
            None => Ok(icon),
        }
    }
}
