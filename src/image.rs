//! A picture in memory: four bytes per pixel in the order blue, green, red, opacity, rows
//! from the top. This is the order in which Windows gives the screen, and the pictures here
//! are always opaque.

use crate::geometry::Rect;

#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// The number of bytes in a row.
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// The picture as a PNG file. The fourth byte isn't written: the pictures are opaque.
    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        let mut file = Vec::new();
        let mut encoder = png::Encoder::new(&mut file, self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let rgb: Vec<u8> = self.pixels.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgb).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        Ok(file)
    }

    /// The part of the picture inside `rect`, which must lie within it.
    pub fn crop(&self, rect: Rect) -> Image {
        debug_assert_eq!(rect.intersect(self.bounds()), rect);
        let row = rect.width() as usize * 4;
        let mut pixels = Vec::with_capacity(row * rect.height() as usize);
        for y in rect.top..rect.bottom {
            let start = y as usize * self.stride() + rect.left as usize * 4;
            pixels.extend_from_slice(&self.pixels[start..start + row]);
        }
        Image { width: rect.width() as u32, height: rect.height() as u32, pixels }
    }
}
