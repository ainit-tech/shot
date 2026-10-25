//! Taking a picture of the whole screen: all monitors at once, in their real pixels, with where
//! the windows on it are.

use std::io;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{BitBlt, CAPTUREBLT, EnumDisplayMonitors, GetDC, HDC, HMONITOR, ReleaseDC, SRCCOPY};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetLayeredWindowAttributes, GetSystemMetrics, GetWindowLongW, GetWindowRect,
    IsIconic, IsWindowVisible, LWA_ALPHA, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows_sys::core::BOOL;

use super::gdi::Buffer;
use super::last_error;
use crate::geometry::{Point, Rect};
use crate::image::Image;

/// A monitor in the picture of the screen.
pub struct Monitor {
    /// Where it is in the picture.
    pub rect: Rect,
    /// Its scale in the Windows settings: 1.5 for 150%.
    pub scale: f32,
}

/// The frozen screen.
pub struct Screen {
    /// Where the picture's top left corner is on the desktop. Monitors to the left of or above
    /// the main one have negative coordinates there.
    pub origin: Point,
    pub image: Image,
    pub monitors: Vec<Monitor>,
    /// The windows on the screen, front to back, as their frames show in the picture.
    pub windows: Vec<Rect>,
}

impl Screen {
    /// The monitor that has the point, or the first one if none has it.
    pub fn monitor_at(&self, p: Point) -> &Monitor {
        self.monitors.iter().find(|m| m.rect.contains(p)).unwrap_or(&self.monitors[0])
    }

    /// The window seen at the point: the frontmost one that has it.
    pub fn window_at(&self, p: Point) -> Option<Rect> {
        self.windows.iter().copied().find(|w| w.contains(p))
    }
}

pub fn capture() -> io::Result<Screen> {
    let (x, y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let mut buffer = Buffer::new(width as u32, height as u32)?;
    unsafe {
        let screen = GetDC(null_mut());
        // CAPTUREBLT also takes windows that are drawn with transparency.
        let copied = BitBlt(buffer.dc(), 0, 0, width, height, screen, x, y, SRCCOPY | CAPTUREBLT);
        let error = (copied == 0).then(last_error);
        ReleaseDC(null_mut(), screen);
        if let Some(error) = error {
            return Err(error);
        }
    }
    let mut pixels = buffer.frame().pixels.to_vec();
    // The screen comes with no opacity at all; it is made opaque for drawing over it.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    let origin = Point::new(x, y);
    let image = Image { width: width as u32, height: height as u32, pixels };
    let mut monitors = monitors(origin);
    if monitors.is_empty() {
        monitors.push(Monitor { rect: image.bounds(), scale: 1.0 });
    }
    let windows = windows(origin, image.bounds());
    Ok(Screen { origin, image, monitors, windows })
}

/// The windows a click on the frozen screen can select, front to back, as their frames show:
/// without the frame Windows keeps invisible around them for resizing. Hidden, minimised and
/// cloaked windows, windows that let clicks through or can't be seen, tool windows, such as
/// overlays and the taskbar, and the desktop itself are left out.
fn windows(origin: Point, bounds: Rect) -> Vec<Rect> {
    unsafe extern "system" fn found(window: HWND, list: LPARAM) -> BOOL {
        let list = unsafe { &mut *(list as *mut Vec<HWND>) };
        list.push(window);
        1
    }
    let mut all: Vec<HWND> = Vec::new();
    unsafe { EnumWindows(Some(found), &mut all as *mut _ as LPARAM) };
    all.into_iter()
        .filter_map(frame)
        .map(|r| Rect::new(r.left, r.top, r.right, r.bottom).offset(-origin.x, -origin.y).intersect(bounds))
        .filter(|r| r.width() >= 8 && r.height() >= 8)
        .collect()
}

/// Where the window shows on the desktop, if a click should be able to select it.
fn frame(window: HWND) -> Option<RECT> {
    unsafe {
        if IsWindowVisible(window) == 0 || IsIconic(window) != 0 {
            return None;
        }
        let style = GetWindowLongW(window, GWL_EXSTYLE) as u32;
        if style & (WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW) != 0 {
            return None;
        }
        if style & WS_EX_LAYERED != 0 {
            let (mut alpha, mut flags) = (255u8, 0u32);
            let known = GetLayeredWindowAttributes(window, null_mut(), &mut alpha, &mut flags) != 0;
            if known && flags & LWA_ALPHA != 0 && alpha == 0 {
                return None;
            }
        }
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(window, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), size_of::<u32>() as u32);
        if cloaked != 0 {
            return None;
        }
        let mut class = [0u16; 32];
        let len = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        if matches!(String::from_utf16_lossy(&class[..len]).as_str(), "Progman" | "WorkerW") {
            return None;
        }
        let mut rect = RECT::default();
        let found = DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut rect as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        );
        if found != 0 && GetWindowRect(window, &mut rect) == 0 {
            return None;
        }
        Some(rect)
    }
}

fn monitors(origin: Point) -> Vec<Monitor> {
    unsafe extern "system" fn found(monitor: HMONITOR, _: HDC, rect: *mut RECT, list: LPARAM) -> BOOL {
        let list = unsafe { &mut *(list as *mut Vec<(HMONITOR, RECT)>) };
        list.push((monitor, unsafe { *rect }));
        1
    }
    let mut found_monitors: Vec<(HMONITOR, RECT)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(found), &mut found_monitors as *mut _ as LPARAM);
    }
    found_monitors
        .into_iter()
        .map(|(monitor, r)| {
            let (mut dpi, mut unused) = (96, 96);
            unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) };
            Monitor {
                rect: Rect::new(r.left, r.top, r.right, r.bottom).offset(-origin.x, -origin.y),
                scale: dpi as f32 / 96.0,
            }
        })
        .collect()
}
