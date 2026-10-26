//! The icon in the notification area. A click on it takes a screenshot; its menu also opens the
//! settings, the presentation of how to use the program and the site's page where people support
//! it, and closes the program. It is also where the program tells the user about problems and
//! news, such as a screenshot saved.

use std::io;
use std::mem::transmute;
use std::ptr::{self, null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_LARGE_ICON, NIIF_NOSOUND, NIIF_USER, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, HICON, ICON_BIG,
    ICON_SMALL, ICONINFO, MF_SEPARATOR, MF_STRING, PostMessageW, SM_CXICON, SM_CXSMICON, SPI_GETWORKAREA, SendMessageW,
    SetForegroundWindow, SystemParametersInfoW, TPM_BOTTOMALIGN, TPM_LAYOUTRTL, TPM_NOANIMATION, TPM_RETURNCMD,
    TPM_RIGHTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_NULL, WM_SETICON,
};
use windows_sys::core::PCSTR;
use windows_sys::w;

use super::gdi::bitmap_info;
use super::{QUIET, last_error, shell, wide};
use crate::draw;
use crate::text::{self, tr};

/// The message the icon sends to the program's window.
pub const MESSAGE: u32 = WM_APP + 1;
const ID: u32 = 1;

pub enum Command {
    Capture,
    Settings,
    Guide,
    Update,
    Support,
    Exit,
}

const CAPTURE: usize = 1;
const SETTINGS: usize = 2;
const EXIT: usize = 3;
const GUIDE: usize = 4;
const UPDATE: usize = 5;
const SUPPORT: usize = 6;

pub struct Tray {
    window: HWND,
    icon: HICON,
    /// The program's icon at the size notifications show it, if it could be drawn.
    big: HICON,
}

impl Tray {
    pub fn add(window: HWND) -> io::Result<Tray> {
        let big = create_icon_of(unsafe { GetSystemMetricsForDpi(SM_CXICON, GetDpiForSystem()) }).unwrap_or(null_mut());
        let tray = Tray { window, icon: create_icon()?, big };
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

    /// Shows a notification next to the icon: a problem with the sign of a warning, news with the
    /// program's icon and without a sound.
    pub fn notify(&self, title: &str, text: &str, problem: bool) {
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = match problem {
            true => NIIF_WARNING,
            false if self.big.is_null() => NIIF_INFO | NIIF_NOSOUND,
            false => NIIF_USER | NIIF_LARGE_ICON | NIIF_NOSOUND,
        };
        data.hBalloonIcon = self.big;
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
            if !self.big.is_null() {
                DestroyIcon(self.big);
            }
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
/// the screenshot item, and `update` is the item of updates, while there is one. Google's account
/// isn't here: one signs in once, from the offer as the program starts or from the settings
/// window. The menu is dark or light as Windows is, and goes from right to left in a language
/// written so.
pub fn menu(window: HWND, hotkey: &str, update: Option<&str>) -> Option<Command> {
    track(window, hotkey, update, shell::windows_dark())
}

/// Shows the icon's menu, dark or light.
fn track(window: HWND, hotkey: &str, update: Option<&str>, dark: bool) -> Option<Command> {
    // A combination reads from left to right in every language, as Windows writes it: the mark
    // keeps "Alt+~" from turning into "~+Alt" in a menu from right to left.
    let capture = wide(&format!("{}\t{hotkey}\u{200E}", tr("Take a screenshot")));
    let settings = wide(tr("Settings"));
    let guide = wide(tr("How to use"));
    let update = update.map(wide);
    let support = wide(tr("Support ainitShot"));
    let exit = wide(tr("Exit"));
    unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, CAPTURE, capture.as_ptr());
        AppendMenuW(menu, MF_STRING, SETTINGS, settings.as_ptr());
        AppendMenuW(menu, MF_STRING, GUIDE, guide.as_ptr());
        if let Some(update) = &update {
            AppendMenuW(menu, MF_STRING, UPDATE, update.as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, SUPPORT, support.as_ptr());
        AppendMenuW(menu, MF_STRING, EXIT, exit.as_ptr());
        let mut flags = TPM_RETURNCMD | TPM_RIGHTBUTTON;
        if text::current().right_to_left() {
            flags |= TPM_LAYOUTRTL;
        }
        let mut at = POINT::default();
        if QUIET {
            // The tests show the menu by the notification area, whole at once, and leave the
            // focus where it is.
            at = corner();
            flags |= TPM_RIGHTALIGN | TPM_BOTTOMALIGN | TPM_NOANIMATION;
        } else {
            GetCursorPos(&mut at);
            // Without this the menu doesn't close when the user clicks elsewhere.
            SetForegroundWindow(window);
        }
        dark_menus(dark);
        let chosen = TrackPopupMenu(menu, flags, at.x, at.y, 0, window, null());
        dark_menus(false);
        PostMessageW(window, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match chosen as usize {
            CAPTURE => Some(Command::Capture),
            SETTINGS => Some(Command::Settings),
            GUIDE => Some(Command::Guide),
            UPDATE => Some(Command::Update),
            SUPPORT => Some(Command::Support),
            EXIT => Some(Command::Exit),
            _ => None,
        }
    }
}

/// Makes the program's menus dark, as Windows' own menus in the notification area are when
/// Windows is dark, or light again. Windows has no public call for it: uxtheme's own calls are
/// taken by their numbers, and without them the menus stay light.
fn dark_menus(dark: bool) {
    type Found = unsafe extern "system" fn() -> isize;
    type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
    type FlushMenuThemes = unsafe extern "system" fn();
    static CALLS: OnceLock<Option<(SetPreferredAppMode, FlushMenuThemes)>> = OnceLock::new();
    let calls = CALLS.get_or_init(|| unsafe {
        let uxtheme = LoadLibraryExW(w!("uxtheme.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if uxtheme.is_null() {
            return None;
        }
        let set = GetProcAddress(uxtheme, 135 as PCSTR)?;
        let flush = GetProcAddress(uxtheme, 136 as PCSTR)?;
        Some((transmute::<Found, SetPreferredAppMode>(set), transmute::<Found, FlushMenuThemes>(flush)))
    });
    if let Some((set, flush)) = calls {
        // 2 is always dark; 0 is as the program has it, and its menus are light.
        unsafe {
            set(if dark { 2 } else { 0 });
            flush();
        }
    }
}

/// The corner of the main screen's work area by the notification area.
fn corner() -> POINT {
    let mut work = RECT::default();
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0) };
    POINT { x: work.right - 8, y: work.bottom - 8 }
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::Graphics::Gdi::HDC;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, EndMenu, EnumThreadWindows, GetClassNameW, GetWindowRect, KillTimer, SetTimer,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows_sys::core::BOOL;

    use super::super::gdi::Buffer;
    use super::*;
    use crate::text::Language;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PrintWindow(window: HWND, dc: HDC, flags: u32) -> BOOL;
    }
    /// PrintWindow takes the window as Windows composes it for the screen.
    const PW_RENDERFULLCONTENT: u32 = 2;

    thread_local! {
        /// Where the picture of the menu open now goes.
        static PICTURE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    /// The menu of this thread that is open, by the class of the menus' windows.
    fn open_menu() -> HWND {
        unsafe extern "system" fn found(window: HWND, menu: LPARAM) -> BOOL {
            let mut class = [0u16; 16];
            let len = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) } as usize;
            if String::from_utf16_lossy(&class[..len]) != "#32768" {
                return 1;
            }
            unsafe { *(menu as *mut HWND) = window };
            0
        }
        let mut menu: HWND = null_mut();
        unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(found), &mut menu as *mut HWND as LPARAM) };
        menu
    }

    /// The timer, inside the menu's own loop: a picture of the menu as Windows shows it, and the
    /// menu closes.
    unsafe extern "system" fn take(window: HWND, _: u32, timer: usize, _: u32) {
        unsafe { KillTimer(window, timer) };
        let menu = open_menu();
        if let (false, Some(path)) = (menu.is_null(), PICTURE.with_borrow_mut(Option::take)) {
            let mut rect = RECT::default();
            unsafe { GetWindowRect(menu, &mut rect) };
            let (width, height) = ((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32);
            let mut buffer = Buffer::new(width, height).unwrap();
            unsafe { PrintWindow(menu, buffer.dc(), PW_RENDERFULLCONTENT) };
            let pixels = buffer.frame().pixels.to_vec();
            let outside = outside_corners(&pixels, width as usize, height as usize);
            let rgba: Vec<u8> = pixels
                .chunks_exact(4)
                .zip(outside)
                .flat_map(|(p, outside)| [p[2], p[1], p[0], if outside { 0 } else { 255 }])
                .collect();
            let mut file = Vec::new();
            let mut encoder = png::Encoder::new(&mut file, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&rgba).unwrap();
            writer.finish().unwrap();
            std::fs::write(path, file).unwrap();
        }
        unsafe { EndMenu() };
    }

    /// The pixels outside the rounded corners of a window, which Windows gives black: those black
    /// ones that reach a corner of the picture, near it.
    fn outside_corners(pixels: &[u8], width: usize, height: usize) -> Vec<bool> {
        let black = |x: usize, y: usize| pixels[(y * width + x) * 4..][..3] == [0, 0, 0];
        let mut outside = vec![false; width * height];
        let near = 16.min(width / 2).min(height / 2);
        for (cx, cy) in [(0, 0), (width - 1, 0), (0, height - 1), (width - 1, height - 1)] {
            let mut next = vec![(cx, cy)];
            while let Some((x, y)) = next.pop() {
                if outside[y * width + x] || !black(x, y) || x.abs_diff(cx) >= near || y.abs_diff(cy) >= near {
                    continue;
                }
                outside[y * width + x] = true;
                let around = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
                next.extend(around.into_iter().filter(|&(x, y)| x < width && y < height));
            }
        }
        outside
    }

    /// Shows the icon's menu for half a second each, in Russian and in Arabic, dark and light, and
    /// with the item of an update, and saves pictures of it into the folder `SCREENSHOT_PREVIEW`:
    /// `SCREENSHOT_PREVIEW=<folder> cargo test -p ainitshot menu_in_pictures -- --ignored`. The menu
    /// shows by the notification area and leaves the focus where it is.
    #[test]
    #[ignore]
    fn the_menu_in_pictures() {
        let folder = PathBuf::from(std::env::var_os("SCREENSHOT_PREVIEW").expect("a folder for the pictures"));
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let window = unsafe {
            CreateWindowExW(WS_EX_TOOLWINDOW, w!("STATIC"), w!(""), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), null_mut(), null())
        };
        let shown = [
            (Language::RU, true, None),
            (Language::RU, false, None),
            (Language::AR, true, None),
            (Language::AR, false, None),
            (Language::RU, true, Some(update_to(Language::RU))),
        ];
        let mut taken = Vec::new();
        for (language, dark, update) in shown {
            text::set(language);
            let theme = if dark { "dark" } else { "light" };
            let more = if update.is_some() { "-update" } else { "" };
            let path = folder.join(format!("menu-{}-{theme}{more}.png", language.code()));
            let _ = std::fs::remove_file(&path);
            PICTURE.set(Some(path.clone()));
            unsafe { SetTimer(window, 1, 500, Some(take)) };
            let chosen = track(window, "Alt+~", update.as_deref(), dark);
            taken.push((path.exists(), chosen.is_none()));
        }
        unsafe { DestroyWindow(window) };
        assert_eq!(taken, [(true, true); 5]);
    }

    /// The item of an update to a version made up for the pictures.
    fn update_to(language: Language) -> String {
        text::tr_in(language, "Update to {version}").replace("{version}", "0.3.0")
    }
}
