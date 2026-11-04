//! What a recording sees of the screen, over and over. The graphics card hands the picture of a
//! monitor over by itself (Desktop Duplication), which leaves the mouse alone and keeps up on
//! large screens. Where it can't, across two monitors, on a monitor turned on its side or in a
//! remote session, GDI takes the picture, as the frozen screen does: it is slower, and the
//! mouse may flicker while it reads the screen. The mouse pointer is not in the picture either
//! way, so it is drawn over it here, on the marks of its clicks.

use std::io;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HMODULE, RECT as ComRect};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1, IDXGIFactory1,
    IDXGIOutput, IDXGIOutput1, IDXGIOutput5, IDXGIOutputDuplication,
};
use windows_core::Interface;
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CAPTUREBLT, DeleteObject, GetDC, HALFTONE, ReleaseDC, SRCCOPY, SetBrushOrgEx, SetStretchBltMode, StretchBlt,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetCursorPos, GetIconInfo, HCURSOR, ICONINFO,
};

use super::gdi::Buffer;
use crate::clicks::Clicks;
use crate::draw::{self, Frame};
use crate::geometry::{Point, Rect};

/// How long to wait for the first picture of a monitor, and how often to ask for it again while
/// another desktop has taken over: a password prompt of Windows.
const FIRST_LOOK: u32 = 300;
const LOOK_AGAIN: Duration = Duration::from_secs(1);

/// The picture of an area of the screen, with the mouse pointer on it.
pub struct Grab {
    /// The area on the desktop.
    area: Rect,
    buffer: Buffer,
    /// The graphics card's way, while it serves.
    card: Option<Card>,
    /// The pointer drawn last and its hot spot.
    pointer: Option<(HCURSOR, POINT)>,
    clicks: Clicks,
    /// The panel of the recording: a click on it is not a part of what is shown.
    panel: Rect,
    /// The scale of the screen in the picture.
    scale: f32,
}

impl Grab {
    /// Sees `area` of the desktop as a picture of `size`, smaller than the area if the area is
    /// too large for a video; `scale` is that of the screen, and `panel` where the panel is.
    pub fn new(area: Rect, size: (u32, u32), scale: f32, panel: Rect) -> io::Result<Grab> {
        let buffer = Buffer::new(size.0, size.1)?;
        let same_size = (area.width() as u32, area.height() as u32) == size;
        let card = same_size.then(|| Card::new(area)).and_then(|card| match card {
            Ok(card) => Some(card),
            Err(problem) => {
                eprintln!("The graphics card doesn't hand the screen over: {problem}");
                None
            }
        });
        let scale = scale * size.0 as f32 / area.width().max(1) as f32;
        Ok(Grab { area, buffer, card, pointer: None, clicks: Clicks::default(), panel, scale })
    }

    /// Forgets the clicks before the recording.
    pub fn forget_clicks(&self) {
        unsafe {
            GetAsyncKeyState(VK_LBUTTON as i32);
            GetAsyncKeyState(VK_RBUTTON as i32);
        }
    }

    /// The way the screen is seen, for what is reported of a recording.
    pub fn way(&self) -> &'static str {
        if self.card.is_some() { "the graphics card" } else { "GDI" }
    }

    /// The picture of this moment, `time` into the recording: four bytes a pixel, rows from the
    /// top.
    pub fn take(&mut self, time: Duration) -> io::Result<Frame<'_>> {
        let taken = match &mut self.card {
            Some(card) => card.take(&mut self.buffer).map_err(io::Error::other),
            None => self.read_screen(),
        };
        if let Err(problem) = taken {
            // The card has gone wrong: GDI goes on with the recording.
            if self.card.take().is_none() {
                return Err(problem);
            }
            eprintln!("The graphics card stopped handing the screen over: {problem}");
            self.read_screen()?;
        }
        self.mark_clicks(time);
        self.draw_pointer();
        Ok(self.buffer.frame())
    }

    /// Where a point of the desktop is in the picture. Across a picture smaller than the area,
    /// the mouse goes as much slower.
    fn in_picture(&mut self, point: POINT) -> (f32, f32) {
        let frame = self.buffer.frame();
        let across = |at: i32, start: i32, length: i32, size: u32| (at - start) as f32 * size as f32 / length.max(1) as f32;
        (across(point.x, self.area.left, self.area.width(), frame.width), across(point.y, self.area.top, self.area.height(), frame.height))
    }

    /// Looks at the mouse's buttons, and draws the marks of its clicks over the picture.
    fn mark_clicks(&mut self, time: Duration) {
        let mut mouse = POINT::default();
        let buttons = unsafe {
            GetCursorPos(&mut mouse);
            GetAsyncKeyState(VK_LBUTTON as i32) as u16 | GetAsyncKeyState(VK_RBUTTON as i32) as u16
        };
        let free = !self.panel.contains(Point::new(mouse.x, mouse.y));
        let (pressed, down) = (buttons & 1 != 0 && free, buttons & 0x8000 != 0 && free);
        let at = self.in_picture(mouse);
        let seconds = time.as_secs_f32();
        self.clicks.update(seconds, at, pressed, down);
        let marks = self.clicks.marks(seconds, self.scale);
        let mut frame = self.buffer.frame();
        for mark in &marks {
            draw::click_mark(&mut frame, mark, self.scale);
        }
    }

    fn read_screen(&mut self) -> io::Result<()> {
        let (width, height) = {
            let frame = self.buffer.frame();
            (frame.width as i32, frame.height as i32)
        };
        let (left, top) = (self.area.left, self.area.top);
        let target = self.buffer.dc();
        let read = unsafe {
            let screen = GetDC(null_mut());
            let read = if (self.area.width(), self.area.height()) == (width, height) {
                BitBlt(target, 0, 0, width, height, screen, left, top, SRCCOPY | CAPTUREBLT)
            } else {
                SetStretchBltMode(target, HALFTONE);
                SetBrushOrgEx(target, 0, 0, null_mut());
                let (w, h) = (self.area.width(), self.area.height());
                StretchBlt(target, 0, 0, width, height, screen, left, top, w, h, SRCCOPY | CAPTUREBLT)
            };
            ReleaseDC(null_mut(), screen);
            read
        };
        if read == 0 { Err(super::last_error()) } else { Ok(()) }
    }

    /// Draws the mouse pointer over the picture, where it is and as it looks now.
    fn draw_pointer(&mut self) {
        let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
        if unsafe { GetCursorInfo(&mut info) } == 0 || info.flags & CURSOR_SHOWING == 0 || info.hCursor.is_null() {
            return;
        }
        let spot = match self.pointer {
            Some((pointer, spot)) if pointer == info.hCursor => spot,
            _ => {
                let spot = hot_spot(info.hCursor);
                self.pointer = Some((info.hCursor, spot));
                spot
            }
        };
        let (x, y) = self.in_picture(info.ptScreenPos);
        unsafe { DrawIconEx(self.buffer.dc(), x as i32 - spot.x, y as i32 - spot.y, info.hCursor, 0, 0, 0, null_mut(), DI_NORMAL) };
    }
}

/// The point of a mouse pointer that is where the mouse is.
fn hot_spot(pointer: HCURSOR) -> POINT {
    let mut info = ICONINFO::default();
    if unsafe { GetIconInfo(pointer, &mut info) } == 0 {
        return POINT::default();
    }
    unsafe {
        DeleteObject(info.hbmMask);
        DeleteObject(info.hbmColor);
    }
    POINT { x: info.xHotspot as i32, y: info.yHotspot as i32 }
}

/// The picture of a monitor from the graphics card.
struct Card {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output: IDXGIOutput,
    /// Lost when another desktop takes over or the screen changes, until it can be had again.
    duplication: Option<IDXGIOutputDuplication>,
    /// The latest picture of the area, where the processor can read it.
    copy: ID3D11Texture2D,
    /// The area on the monitor.
    place: D3D11_BOX,
    width: usize,
    height: usize,
    looked: Instant,
}

impl Card {
    fn new(area: Rect) -> windows_core::Result<Card> {
        let (adapter, output, monitor) = output_of(area).ok_or_else(windows_core::Error::empty)?;
        let (mut device, mut context) = (None, None);
        let levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0];
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.ok_or_else(windows_core::Error::empty)?;
        let context = context.ok_or_else(windows_core::Error::empty)?;
        let (width, height) = (area.width() as u32, area.height() as u32);
        let description = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut copy = None;
        unsafe { device.CreateTexture2D(&description, None, Some(&mut copy))? };
        let copy = copy.ok_or_else(windows_core::Error::empty)?;
        let place = D3D11_BOX {
            left: (area.left - monitor.left) as u32,
            top: (area.top - monitor.top) as u32,
            front: 0,
            right: (area.right - monitor.left) as u32,
            bottom: (area.bottom - monitor.top) as u32,
            back: 1,
        };
        let mut card = Card {
            device,
            context,
            output,
            duplication: None,
            copy,
            place,
            width: width as usize,
            height: height as usize,
            looked: Instant::now(),
        };
        card.duplication = Some(card.duplicate()?);
        // The first look gives the whole picture. Without it, the recording would be black until
        // something on the screen changes.
        if !card.look(FIRST_LOOK)? {
            return Err(windows_core::Error::empty());
        }
        Ok(card)
    }

    fn duplicate(&self) -> windows_core::Result<IDXGIOutputDuplication> {
        unsafe {
            let duplication = match self.output.cast::<IDXGIOutput5>() {
                Ok(output) => output.DuplicateOutput1(&self.device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM])?,
                Err(_) => self.output.cast::<IDXGIOutput1>()?.DuplicateOutput(&self.device)?,
            };
            let description = duplication.GetDesc();
            let turned = description.Rotation != DXGI_MODE_ROTATION_IDENTITY && description.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED;
            if turned {
                return Err(windows_core::Error::empty());
            }
            Ok(duplication)
        }
    }

    /// Takes what has come since the last look into the copy, waiting for it for at most `wait`
    /// milliseconds, and says whether anything has: nothing comes while nothing on the screen
    /// changes.
    fn look(&mut self, wait: u32) -> windows_core::Result<bool> {
        let Some(duplication) = &self.duplication else { return Ok(false) };
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        match unsafe { duplication.AcquireNextFrame(wait, &mut info, &mut resource) } {
            Ok(()) => {
                let copied = resource.ok_or_else(windows_core::Error::empty).and_then(|resource| {
                    let picture: ID3D11Texture2D = resource.cast()?;
                    let mut description = D3D11_TEXTURE2D_DESC::default();
                    unsafe { picture.GetDesc(&mut description) };
                    if description.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
                        return Err(windows_core::Error::empty());
                    }
                    unsafe { self.context.CopySubresourceRegion(&self.copy, 0, 0, 0, 0, &picture, 0, Some(&self.place)) };
                    Ok(())
                });
                let released = unsafe { duplication.ReleaseFrame() };
                copied.and(released).map(|()| true)
            }
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => Ok(false),
            Err(error) if error.code() == DXGI_ERROR_ACCESS_LOST => {
                self.duplication = None;
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    /// Brings the buffer the latest picture.
    fn take(&mut self, buffer: &mut Buffer) -> windows_core::Result<()> {
        if self.duplication.is_none() && self.looked.elapsed() >= LOOK_AGAIN {
            self.looked = Instant::now();
            // Until the desktop comes back, the last picture stays.
            self.duplication = self.duplicate().ok();
        }
        self.look(0)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { self.context.Map(&self.copy, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
        let row = self.width * 4;
        let frame = buffer.frame();
        for (y, line) in frame.pixels.chunks_exact_mut(row).take(self.height).enumerate() {
            let source = unsafe { mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize) };
            unsafe { std::ptr::copy_nonoverlapping(source, line.as_mut_ptr(), row) };
        }
        unsafe { self.context.Unmap(&self.copy, 0) };
        Ok(())
    }
}

/// The graphics card and the monitor that have the whole area, with the monitor's place on the
/// desktop. Nothing if it stands on two monitors.
fn output_of(area: Rect) -> Option<(IDXGIAdapter1, IDXGIOutput, ComRect)> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    for number in 0.. {
        let adapter = unsafe { factory.EnumAdapters1(number) }.ok()?;
        for number in 0.. {
            let Ok(output) = (unsafe { adapter.EnumOutputs(number) }) else { break };
            let Ok(description) = (unsafe { output.GetDesc() }) else { continue };
            let monitor = description.DesktopCoordinates;
            let inside = area.left >= monitor.left && area.top >= monitor.top && area.right <= monitor.right && area.bottom <= monitor.bottom;
            if description.AttachedToDesktop.as_bool() && inside {
                return Some((adapter, output, monitor));
            }
        }
    }
    None
}
