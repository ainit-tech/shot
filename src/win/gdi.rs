//! GDI: a picture in memory that the overlay is composed in and copied to the screen from,
//! and text, which GDI draws the way Windows draws it everywhere.

use std::io;
use std::ptr::null_mut;
use std::slice;

use windows_sys::Win32::Foundation::{COLORREF, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC,
    CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    FF_DONTCARE, FONT_QUALITY, FW_NORMAL, FW_SEMIBOLD, GdiFlush, GetTextExtentPoint32W, HBITMAP, HDC, HFONT, HGDIOBJ,
    OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, TextOutW,
};
use windows_sys::core::PCWSTR;
use windows_sys::w;

use super::last_error;
use crate::annotation::Coverage;
use crate::color::Rgba;
use crate::draw::Frame;

/// A picture in memory, four bytes per pixel, rows from the top, with its own device context
/// for GDI to draw on it and copy from it.
pub struct Buffer {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl Buffer {
    pub fn new(width: u32, height: u32) -> io::Result<Buffer> {
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return Err(last_error());
            }
            let info = bitmap_info(width as i32, -(height as i32));
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() {
                let error = last_error();
                DeleteDC(dc);
                return Err(error);
            }
            let previous = SelectObject(dc, bitmap);
            Ok(Buffer { dc, bitmap, previous, bits: bits.cast(), width, height })
        }
    }

    pub fn dc(&self) -> HDC {
        self.dc
    }

    /// The pixels, to draw on them directly. GDI may still be finishing earlier work on them,
    /// so that is waited for first.
    pub fn frame(&mut self) -> Frame<'_> {
        unsafe { GdiFlush() };
        let len = self.width as usize * self.height as usize * 4;
        Frame { pixels: unsafe { slice::from_raw_parts_mut(self.bits, len) }, width: self.width, height: self.height }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// The description of a picture with four bytes per pixel. A negative height means that its
/// rows go from the top.
pub fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The font of the program's labels, Segoe UI Variable, as in Windows 11 itself.
pub struct Font(HFONT);

impl Font {
    pub fn new(pixels: i32, bold: bool) -> io::Result<Font> {
        Font::with_quality(pixels, bold, CLEARTYPE_QUALITY)
    }

    /// The font smoothed in grey, not in the colours of ClearType, for letters that are drawn
    /// in any colour over any picture.
    pub fn grey(pixels: i32, bold: bool) -> io::Result<Font> {
        Font::with_quality(pixels, bold, ANTIALIASED_QUALITY)
    }

    fn with_quality(pixels: i32, bold: bool, quality: FONT_QUALITY) -> io::Result<Font> {
        Font::of(w!("Segoe UI Variable Text"), pixels, bold, quality)
    }

    /// Consolas, the font of terminals, for pictures of what terminals show.
    #[cfg(test)]
    pub fn consolas(pixels: i32) -> io::Result<Font> {
        Font::of(w!("Consolas"), pixels, false, CLEARTYPE_QUALITY)
    }

    fn of(face: PCWSTR, pixels: i32, bold: bool, quality: FONT_QUALITY) -> io::Result<Font> {
        let weight = if bold { FW_SEMIBOLD } else { FW_NORMAL };
        let handle = unsafe {
            CreateFontW(
                -pixels,
                0,
                0,
                0,
                weight as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                quality as u32,
                (DEFAULT_PITCH | FF_DONTCARE) as u32,
                face,
            )
        };
        if handle.is_null() { Err(last_error()) } else { Ok(Font(handle)) }
    }

    /// The font itself, for a control that draws its own text.
    pub fn handle(&self) -> HFONT {
        self.0
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe { DeleteObject(self.0) };
    }
}

/// The width and height of a line of text.
pub fn text_size(dc: HDC, font: &Font, text: &[u16]) -> (i32, i32) {
    let mut size = SIZE::default();
    unsafe {
        let previous = SelectObject(dc, font.0);
        GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut size);
        SelectObject(dc, previous);
    }
    (size.cx, size.cy)
}

/// The letters of text drawn by Windows, as how much they cover each pixel, with `room` pixels
/// around them; its lines go one under another. Also where the caret goes at byte `caret` of the
/// text: its left side, top and height.
pub fn letters(text: &str, size: i32, room: i32, caret: usize) -> io::Result<(Coverage, (i32, i32, i32))> {
    let font = Font::grey(size, true)?;
    let lines: Vec<Vec<u16>> = text.split('\n').map(|line| line.encode_utf16().collect()).collect();
    let measuring = Buffer::new(1, 1)?;
    let widths: Vec<i32> = lines.iter().map(|line| text_size(measuring.dc(), &font, line).0).collect();
    let line_height = text_size(measuring.dc(), &font, &[u16::from(b'A')]).1.max(1);
    let width = widths.iter().copied().max().unwrap_or(0).max(1) + 2 * room;
    let height = line_height * lines.len() as i32 + 2 * room;
    let mut buffer = Buffer::new(width as u32, height as u32)?;
    buffer.frame().pixels.fill(0);
    for (i, line) in lines.iter().enumerate() {
        draw_text(buffer.dc(), &font, line, room, room + i as i32 * line_height, [255, 255, 255, 255]);
    }
    // White letters on black: any of the colours says how much a pixel is covered.
    let values = buffer.frame().pixels.chunks_exact(4).map(|pixel| pixel[1]).collect();
    // The caret goes after what its line has before it.
    let before = text.get(..caret).unwrap_or(text);
    let line = before.matches('\n').count() as i32;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let prefix: Vec<u16> = before[start..].encode_utf16().collect();
    let x = if prefix.is_empty() { 0 } else { text_size(measuring.dc(), &font, &prefix).0 };
    let caret = (room + x, room + line * line_height, line_height);
    Ok((Coverage { width: width as u32, height: height as u32, values }, caret))
}

/// Draws a line of text with its top left corner at the given point.
pub fn draw_text(dc: HDC, font: &Font, text: &[u16], x: i32, y: i32, color: Rgba) {
    let [red, green, blue, _] = color;
    let color: COLORREF = red as u32 | (green as u32) << 8 | (blue as u32) << 16;
    unsafe {
        let previous = SelectObject(dc, font.0);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        TextOutW(dc, x, y, text.as_ptr(), text.len() as i32);
        SelectObject(dc, previous);
    }
}

#[cfg(test)]
mod tests {
    use tiny_skia::Pixmap;

    use super::*;
    use crate::annotation::{self, Annotation, Label, Shape};
    use crate::draw;
    use crate::geometry::Point;
    use crate::image::Image;

    #[test]
    fn the_caret_stands_before_the_letter_it_is_at() {
        let text = "ab\ncd";
        let caret = |at| letters(text, 20, 3, at).unwrap().1;
        let (start, middle, end) = (caret(0), caret(1), caret(2));
        assert_eq!(start.0, 3);
        assert!(start.0 < middle.0 && middle.0 < end.0);
        assert_eq!((start.1, middle.1, end.1), (3, 3, 3));
        // The second line begins under the first one.
        let second = caret(3);
        assert_eq!((second.0, second.1), (3, 3 + second.2));
        assert_eq!(caret(5).1, second.1);
        assert!(caret(4).0 < caret(5).0);
    }

    /// Writes labels in the palette's colours over a light and a dark background, as a picture
    /// to look at: `SCREENSHOT_PREVIEW=<file.png> cargo test -p ainitshot labels -- --ignored`.
    #[test]
    #[ignore]
    fn labels_are_readable_on_light_and_dark() {
        let (width, height) = (760u32, 300u32);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let light = y < height / 2;
                let stripe = (x / 40) % 2 == 0;
                let shade: u8 = match (light, stripe) {
                    (true, true) => 245,
                    (true, false) => 214,
                    (false, true) => 32,
                    (false, false) => 70,
                };
                pixels.extend_from_slice(&[shade, shade, shade, 255]);
            }
        }
        let mut picture = Image { width, height, pixels };
        let colors = [[229, 57, 53, 255], [253, 216, 53, 255], [67, 160, 71, 255], [30, 136, 229, 255], [33, 33, 33, 255], [255, 255, 255, 255]];
        let size = annotation::text_size(4.0);
        let room = annotation::text_margin(size);
        let mut labels = Vec::new();
        for (i, color) in colors.iter().enumerate() {
            for half in 0..2 {
                let text = match half {
                    0 => ["看这里", "यहाँ देखें", "انظر هنا"][i % 3],
                    _ => ["Click here", "ดูตรงนี้", "ဒီမှာကြည့်", "እዚህ ተመልከት", "აქ შეხედე", "এখানে দেখুন"][i],
                };
                let (coverage, _) = letters(text, size as i32, room, text.len()).unwrap();
                let at = Point::new(20 + (i as i32 % 3) * 250, 20 + (i as i32 / 3) * 60 + half * 150);
                let label = Label { text: text.into(), size, at, coverage };
                labels.push(Annotation { shape: Shape::Text(label), color: *color, width: 4.0 });
            }
        }
        let mut layer = Pixmap::new(width, height).unwrap();
        draw::annotations(&mut layer, &labels, Point::new(0, 0));
        let bounds = picture.bounds();
        draw::blend(&mut picture.pixels, width, &layer, Point::new(0, 0), bounds);
        if let Some(path) = std::env::var_os("SCREENSHOT_PREVIEW") {
            std::fs::write(path, picture.to_png().unwrap()).unwrap();
        }
    }
}
