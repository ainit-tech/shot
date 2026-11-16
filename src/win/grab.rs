//! What a recording sees of the screen, over and over. The graphics card hands the picture of a
//! monitor over by itself (Desktop Duplication), which leaves the mouse alone and keeps up on
//! large screens. Where it can't, across two monitors, on a monitor turned on its side or in a
//! remote session, GDI takes the picture, as the frozen screen does: it is slower, and the
//! mouse may flicker while it reads the screen. The mouse pointer is not in the picture either
//! way, so it is drawn over it here, on the marks of its clicks.
//!
//! For the card's own encoder the picture stays on the card: only the small part under the
//! pointer and the marks comes to the processor to be drawn on, and goes back.

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
    BITMAP, BitBlt, CAPTUREBLT, DeleteObject, GetDC, GetObjectW, HALFTONE, ReleaseDC, SRCCOPY, SetBrushOrgEx, SetStretchBltMode,
    StretchBlt,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetCursorPos, GetIconInfo, HCURSOR, ICONINFO,
};

use super::encoder;
use super::gdi::Buffer;
use crate::clicks::{Clicks, Mark};
use crate::draw::{self, Frame};
use crate::geometry::{Point, Rect};

/// How long to wait for the first picture of a monitor, and how often to ask for it again while
/// another desktop has taken over: a password prompt of Windows.
const FIRST_LOOK: u32 = 300;
const LOOK_AGAIN: Duration = Duration::from_secs(1);
/// How far a mark of a click reaches past its radius, at the scale of 100%: the pale line under
/// a ring, with room to spare.
const MARK_REACH: f32 = 4.0;

/// The picture of an area of the screen, with the mouse pointer on it.
pub struct Grab {
    /// The area on the desktop.
    area: Rect,
    /// The size of the pictures: the video's in memory, the area's on the card, which makes it
    /// the video's itself.
    size: (u32, u32),
    buffer: Buffer,
    /// The graphics card's way, while it serves.
    card: Option<Card>,
    /// The pointer drawn last.
    pointer: Option<Pointer>,
    clicks: Clicks,
    /// The panel of the recording: a click on it is not a part of what is shown.
    panel: Rect,
    /// The scale of the screen in the picture.
    scale: f32,
}

/// The picture of a moment.
pub enum Picture<'a> {
    /// In memory: four bytes a pixel, rows from the top.
    Memory(Frame<'a>),
    /// On the graphics card.
    Card(&'a ID3D11Texture2D),
}

/// A pointer and where it is in the picture.
type Placed = (Pointer, (f32, f32));

/// A mouse pointer as it looks: the point of it that is where the mouse is, and its size.
#[derive(Clone, Copy)]
struct Pointer {
    look: HCURSOR,
    spot: POINT,
    size: (i32, i32),
}

impl Grab {
    /// Sees `area` of the desktop as a picture of `size`, smaller than the area if the area is
    /// too large for a video; `scale` is that of the screen, and `panel` where the panel is. With
    /// `on_card` the pictures stay on the graphics card, if it hands the screen over.
    pub fn new(area: Rect, size: (u32, u32), scale: f32, panel: Rect, on_card: bool) -> io::Result<Grab> {
        let whole = (area.width() as u32, area.height() as u32);
        let card = (on_card || whole == size).then(|| Card::new(area, on_card)).and_then(|card| match card {
            Ok(card) => Some(card),
            Err(problem) => {
                eprintln!("The graphics card doesn't hand the screen over: {problem}");
                None
            }
        });
        // For the card's encoder, a picture from memory has the area's size too.
        let size = if on_card { whole } else { size };
        let buffer = Buffer::new(size.0, size.1)?;
        let scale = scale * size.0 as f32 / area.width().max(1) as f32;
        Ok(Grab { area, size, buffer, card, pointer: None, clicks: Clicks::default(), panel, scale })
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

    /// The size of the pictures.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The graphics card the pictures stay on, for its encoder to take them there.
    pub fn device(&self) -> Option<ID3D11Device> {
        self.card.as_ref().filter(|card| card.drawn.is_some()).map(|card| card.device.clone())
    }

    /// The picture of this moment, `time` into the recording.
    pub fn take(&mut self, time: Duration) -> io::Result<Picture<'_>> {
        let taken = match &mut self.card {
            Some(card) if card.drawn.is_some() => card.refresh().map_err(io::Error::other),
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
        let marks = self.marks(time);
        let pointer = self.pointer();
        let scale = self.scale;
        if let Some(card) = self.card.as_mut().filter(|card| card.drawn.is_some()) {
            let place = reach(&marks, pointer, scale).intersect(Rect::new(0, 0, self.size.0 as i32, self.size.1 as i32));
            let drawn = card.draw(place, |patch, origin| draw_over(patch, origin, &marks, pointer, scale));
            match drawn {
                // The borrow of the card ends with the picture it gives.
                Ok(()) => return Ok(Picture::Card(self.card.as_ref().and_then(Card::drawn).ok_or_else(|| io::Error::other("no picture"))?)),
                Err(problem) => {
                    eprintln!("The graphics card stopped handing the screen over: {problem}");
                    self.card = None;
                    self.read_screen()?;
                }
            }
        }
        draw_over(&mut self.buffer, (0, 0), &marks, pointer, scale);
        Ok(Picture::Memory(self.buffer.frame()))
    }

    /// Where a point of the desktop is in the picture. Across a picture smaller than the area,
    /// the mouse goes as much slower.
    fn in_picture(&self, point: POINT) -> (f32, f32) {
        let across = |at: i32, start: i32, length: i32, size: u32| (at - start) as f32 * size as f32 / length.max(1) as f32;
        (across(point.x, self.area.left, self.area.width(), self.size.0), across(point.y, self.area.top, self.area.height(), self.size.1))
    }

    /// Looks at the mouse's buttons: the marks of its clicks to draw over the picture.
    fn marks(&mut self, time: Duration) -> Vec<Mark> {
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
        self.clicks.marks(seconds, self.scale)
    }

    fn read_screen(&mut self) -> io::Result<()> {
        let (width, height) = (self.size.0 as i32, self.size.1 as i32);
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

    /// The mouse pointer, where it is in the picture and as it looks now, if it shows.
    fn pointer(&mut self) -> Option<Placed> {
        let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
        if unsafe { GetCursorInfo(&mut info) } == 0 || info.flags & CURSOR_SHOWING == 0 || info.hCursor.is_null() {
            return None;
        }
        let pointer = match self.pointer {
            Some(pointer) if pointer.look == info.hCursor => pointer,
            _ => {
                let pointer = Pointer::of(info.hCursor);
                self.pointer = Some(pointer);
                pointer
            }
        };
        Some((pointer, self.in_picture(info.ptScreenPos)))
    }
}

impl Pointer {
    fn of(look: HCURSOR) -> Pointer {
        let mut info = ICONINFO::default();
        if unsafe { GetIconInfo(look, &mut info) } == 0 {
            return Pointer { look, spot: POINT::default(), size: (32, 32) };
        }
        let size = |bitmap| {
            let mut description = BITMAP::default();
            let read = unsafe { GetObjectW(bitmap, size_of::<BITMAP>() as i32, (&raw mut description).cast()) };
            (read != 0).then_some((description.bmWidth, description.bmHeight))
        };
        // A pointer of two colours has its mask twice as tall as itself, the second half being its picture.
        let size = match info.hbmColor.is_null() {
            false => size(info.hbmColor),
            true => size(info.hbmMask).map(|(width, height)| (width, height / 2)),
        };
        unsafe {
            DeleteObject(info.hbmMask);
            DeleteObject(info.hbmColor);
        }
        Pointer { look, spot: POINT { x: info.xHotspot as i32, y: info.yHotspot as i32 }, size: size.unwrap_or((32, 32)) }
    }
}

/// The part of the picture that the marks and the pointer cover.
fn reach(marks: &[Mark], pointer: Option<Placed>, scale: f32) -> Rect {
    let mut place = Rect::default();
    for mark in marks {
        let reach = mark.radius + MARK_REACH * scale + 2.0;
        let covered = Rect::new((mark.x - reach).floor() as i32, (mark.y - reach).floor() as i32, (mark.x + reach).ceil() as i32, (mark.y + reach).ceil() as i32);
        place = place.union(covered);
    }
    if let Some((pointer, (x, y))) = pointer {
        let (left, top) = (x as i32 - pointer.spot.x, y as i32 - pointer.spot.y);
        place = place.union(Rect::new(left, top, left + pointer.size.0, top + pointer.size.1));
    }
    place
}

/// Draws the marks of clicks and the pointer over a picture whose top left corner is at `origin`
/// of the whole one.
fn draw_over(picture: &mut Buffer, origin: (i32, i32), marks: &[Mark], pointer: Option<Placed>, scale: f32) {
    {
        let mut frame = picture.frame();
        for mark in marks {
            let moved = Mark { x: mark.x - origin.0 as f32, y: mark.y - origin.1 as f32, ..*mark };
            draw::click_mark(&mut frame, &moved, scale);
        }
    }
    if let Some((pointer, (x, y))) = pointer {
        let (left, top) = (x as i32 - pointer.spot.x - origin.0, y as i32 - pointer.spot.y - origin.1);
        unsafe { DrawIconEx(picture.dc(), left, top, pointer.look, 0, 0, 0, null_mut(), DI_NORMAL) };
    }
}

/// The picture of a monitor from the graphics card.
struct Card {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output: IDXGIOutput,
    /// Lost when another desktop takes over or the screen changes, until it can be had again.
    duplication: Option<IDXGIOutputDuplication>,
    /// The latest picture of the area: where the processor can read it, or on the card.
    copy: ID3D11Texture2D,
    /// On the card: the picture with the pointer and the marks on it.
    drawn: Option<Overlay>,
    /// The area on the monitor.
    place: D3D11_BOX,
    width: usize,
    height: usize,
    looked: Instant,
}

impl Card {
    fn new(area: Rect, on_card: bool) -> windows_core::Result<Card> {
        let (adapter, output, monitor) = output_of(area).ok_or_else(windows_core::Error::empty)?;
        let (device, context) = match on_card {
            true => encoder::video_device(Some(&adapter))?,
            false => device_on(&adapter)?,
        };
        let (width, height) = (area.width() as u32, area.height() as u32);
        let (copy, drawn) = match on_card {
            true => (encoder::picture(&device, (width, height))?, Some(Overlay::new(&device, (width, height))?)),
            false => (staging(&device, (width, height))?, None),
        };
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
            drawn,
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

    /// Takes the latest picture into the copy. Until a desktop that has taken over goes, the last
    /// picture stays.
    fn refresh(&mut self) -> windows_core::Result<()> {
        if self.duplication.is_none() && self.looked.elapsed() >= LOOK_AGAIN {
            self.looked = Instant::now();
            self.duplication = self.duplicate().ok();
        }
        self.look(0).map(|_| ())
    }

    /// Brings the buffer the latest picture.
    fn take(&mut self, buffer: &mut Buffer) -> windows_core::Result<()> {
        self.refresh()?;
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

    /// On the card: the latest picture with what `draw` draws over `place` of it.
    fn draw(&mut self, place: Rect, draw: impl FnOnce(&mut Buffer, (i32, i32))) -> windows_core::Result<()> {
        let Some(overlay) = &mut self.drawn else { return Err(windows_core::Error::empty()) };
        overlay.draw(&self.context, &self.copy, place, draw)
    }

    /// The picture drawn on, on the card.
    fn drawn(&self) -> Option<&ID3D11Texture2D> {
        self.drawn.as_ref().map(|overlay| &overlay.drawn)
    }
}

/// A picture of the card with the pointer and the marks of clicks over it: only the part of it
/// they cover is read into memory to be drawn on, and goes back.
struct Overlay {
    drawn: ID3D11Texture2D,
    /// Where that part is read, and that part in memory.
    staging: ID3D11Texture2D,
    patch: Option<Buffer>,
}

impl Overlay {
    fn new(device: &ID3D11Device, size: (u32, u32)) -> windows_core::Result<Overlay> {
        Ok(Overlay { drawn: encoder::picture(device, size)?, staging: staging(device, size)?, patch: None })
    }

    /// `picture` with what `draw` draws over `place` of it.
    fn draw(
        &mut self,
        context: &ID3D11DeviceContext,
        picture: &ID3D11Texture2D,
        place: Rect,
        draw: impl FnOnce(&mut Buffer, (i32, i32)),
    ) -> windows_core::Result<()> {
        unsafe { context.CopyResource(&self.drawn, picture) };
        if place.is_empty() {
            return Ok(());
        }
        let (width, height) = (place.width() as u32, place.height() as u32);
        let part = D3D11_BOX { left: place.left as u32, top: place.top as u32, front: 0, right: place.right as u32, bottom: place.bottom as u32, back: 1 };
        unsafe { context.CopySubresourceRegion(&self.staging, 0, 0, 0, 0, picture, 0, Some(&part)) };
        let patch = match &mut self.patch {
            Some(patch) if patch.size() == (width, height) => patch,
            patch => patch.insert(Buffer::new(width, height).map_err(|_| windows_core::Error::empty())?),
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
        let row = width as usize * 4;
        for (y, line) in patch.frame().pixels.chunks_exact_mut(row).enumerate() {
            let source = unsafe { mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize) };
            unsafe { std::ptr::copy_nonoverlapping(source, line.as_mut_ptr(), row) };
        }
        unsafe { context.Unmap(&self.staging, 0) };
        draw(patch, (place.left, place.top));
        let pixels = patch.frame().pixels.as_ptr();
        unsafe { context.UpdateSubresource(&self.drawn, 0, Some(&part), pixels.cast(), row as u32, 0) };
        Ok(())
    }
}

/// A picture of the card that the processor can read.
fn staging(device: &ID3D11Device, (width, height): (u32, u32)) -> windows_core::Result<ID3D11Texture2D> {
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
    copy.ok_or_else(windows_core::Error::empty)
}

/// A device of the card for reading the screen into memory.
fn device_on(adapter: &IDXGIAdapter1) -> windows_core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    let (mut device, mut context) = (None, None);
    let levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0];
    unsafe {
        D3D11CreateDevice(
            adapter,
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
    Ok((device.ok_or_else(windows_core::Error::empty)?, context.ok_or_else(windows_core::Error::empty)?))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(x: f32, y: f32, radius: f32) -> Mark {
        Mark { x, y, radius, strength: 1.0, ring: true }
    }

    /// The part read back to be drawn on holds every mark and the whole pointer, and no more.
    #[test]
    fn the_part_drawn_on_holds_the_marks_and_the_pointer() {
        let pointer = Pointer { look: null_mut(), spot: POINT { x: 4, y: 2 }, size: (32, 48) };
        assert!(reach(&[], None, 1.0).is_empty());
        assert_eq!(reach(&[], Some((pointer, (100.0, 50.0))), 1.0), Rect::new(96, 48, 128, 96));
        let ring = reach(&[mark(200.0, 100.0, 30.0)], None, 1.0);
        assert!(ring.left <= 200 - 34 && ring.right >= 200 + 34 && ring.width() < 80, "{ring:?}");
        let both = reach(&[mark(200.0, 100.0, 30.0)], Some((pointer, (100.0, 50.0))), 2.0);
        assert!(both.left == 96 && both.top == 48 && both.right >= 240 && both.bottom >= 140, "{both:?}");
    }

    /// The pointer and the marks drawn on the card, over the part of the picture they cover, are
    /// what the processor draws over the whole picture, to the pixel. A computer without a card
    /// for video has nothing to check.
    #[test]
    fn the_card_draws_the_pointer_and_the_marks_as_the_processor_does() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IDC_ARROW, IDC_IBEAM, LoadCursorW};
        let Ok((device, context)) = encoder::video_device(None) else { return };
        let (width, height) = (300u32, 200u32);
        let mut ground = Buffer::new(width, height).unwrap();
        for (n, pixel) in ground.frame().pixels.chunks_exact_mut(4).enumerate() {
            let (x, y) = (n % width as usize, n / width as usize);
            pixel.copy_from_slice(&[(x * 255 / width as usize) as u8, (y * 255 / height as usize) as u8, ((x + y) % 256) as u8, 255]);
        }
        let picture = encoder::picture(&device, (width, height)).unwrap();
        unsafe { context.UpdateSubresource(&picture, 0, None, ground.frame().pixels.as_ptr().cast(), width * 4, 0) };
        let mut overlay = Overlay::new(&device, (width, height)).unwrap();
        let read = |overlay: &Overlay| -> Vec<u8> {
            let copy = staging(&device, (width, height)).unwrap();
            unsafe { context.CopyResource(&copy, &overlay.drawn) };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe { context.Map(&copy, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap() };
            let row = width as usize * 4;
            let mut pixels = vec![0u8; row * height as usize];
            for (y, line) in pixels.chunks_exact_mut(row).enumerate() {
                let source = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize), row) };
                line.copy_from_slice(source);
            }
            unsafe { context.Unmap(&copy, 0) };
            pixels
        };
        // A ring and a dot by the arrow, and the text pointer at the edge, half out of the picture.
        let arrow = Pointer::of(unsafe { LoadCursorW(null_mut(), IDC_ARROW) });
        let beam = Pointer::of(unsafe { LoadCursorW(null_mut(), IDC_IBEAM) });
        let cases: [(Vec<Mark>, Option<Placed>); 3] = [
            (vec![mark(120.0, 90.0, 26.0), Mark { x: 124.0, y: 92.0, radius: 12.0, strength: 0.8, ring: false }], Some((arrow, (124.0, 92.0)))),
            (vec![], Some((beam, (296.0, 195.0)))),
            (vec![], None),
        ];
        for (marks, pointer) in cases {
            let place = reach(&marks, pointer, 1.0).intersect(Rect::new(0, 0, width as i32, height as i32));
            overlay.draw(&context, &picture, place, |patch, origin| draw_over(patch, origin, &marks, pointer, 1.0)).unwrap();
            let on_card = read(&overlay);
            let mut whole = Buffer::new(width, height).unwrap();
            whole.frame().pixels.copy_from_slice(ground.frame().pixels);
            draw_over(&mut whole, (0, 0), &marks, pointer, 1.0);
            let by_processor = whole.frame().pixels.to_vec();
            // The smoothing of a ring's edge comes out a few steps apart for a circle drawn at
            // another place, nothing more.
            let apart = |a: &[u8], b: &[u8]| a[..3].iter().zip(&b[..3]).map(|(&a, &b)| (a as i32 - b as i32).abs()).max().unwrap_or(0);
            let pairs = || on_card.chunks_exact(4).zip(by_processor.chunks_exact(4));
            let (differ, most) = (pairs().filter(|(a, b)| apart(a, b) > 0).count(), pairs().map(|(a, b)| apart(a, b)).max().unwrap_or(0));
            assert!(differ <= 12 && most <= 16, "{differ} pixels differ, by up to {most}, for {} marks", marks.len());
            // Something was drawn, where there was something to draw.
            let changed = on_card.chunks_exact(4).zip(ground.frame().pixels.chunks_exact(4)).filter(|(a, b)| a[..3] != b[..3]).count();
            assert_eq!(changed > 0, pointer.is_some(), "{changed}");
        }
    }

    /// The marks and the pointer drawn over a part of a picture are what they would be over the
    /// whole of it.
    #[test]
    fn a_part_is_drawn_on_as_the_whole_would_be() {
        let (width, height) = (160u32, 120u32);
        let ground = |buffer: &mut Buffer| {
            for (n, pixel) in buffer.frame().pixels.chunks_exact_mut(4).enumerate() {
                pixel.copy_from_slice(&[(n % 251) as u8, (n % 241) as u8, (n % 239) as u8, 255]);
            }
        };
        let marks = [mark(70.0, 60.0, 20.0), Mark { x: 75.0, y: 58.0, radius: 12.0, strength: 0.7, ring: false }];
        let mut whole = Buffer::new(width, height).unwrap();
        ground(&mut whole);
        draw_over(&mut whole, (0, 0), &marks, None, 1.0);
        let place = reach(&marks, None, 1.0);
        let mut part = Buffer::new(place.width() as u32, place.height() as u32).unwrap();
        {
            let source = whole.frame().pixels.to_vec();
            let mut clean = Buffer::new(width, height).unwrap();
            ground(&mut clean);
            let clean = clean.frame().pixels.to_vec();
            let row = place.width() as usize * 4;
            for (y, line) in part.frame().pixels.chunks_exact_mut(row).enumerate() {
                let from = ((place.top as usize + y) * width as usize + place.left as usize) * 4;
                line.copy_from_slice(&clean[from..from + row]);
            }
            draw_over(&mut part, (place.left, place.top), &marks, None, 1.0);
            for (y, line) in part.frame().pixels.chunks_exact(row).enumerate() {
                let from = ((place.top as usize + y) * width as usize + place.left as usize) * 4;
                assert!(line == &source[from..from + row], "row {y}");
            }
        }
    }
}
