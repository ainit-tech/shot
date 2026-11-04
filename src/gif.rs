//! Animated GIF files, written frame by frame: what is taken from a recording. A frame keeps only
//! what has changed since the frame before, the rest of it transparent, which is what makes a
//! recording of a screen small; a frame with nothing changed lengthens the one before. Every frame
//! has 255 colours of its own, chosen by cutting the colours in it in two, the most numerous and
//! the most different first, and packed with LZW, as the format asks.

use std::io::{self, Write};

/// Colours of a frame, and the one more that says a pixel stays as it was.
const COLOURS: usize = 255;
const TRANSPARENT: u8 = 255;
/// How far apart two pixels may be in a channel and still be the same: video leaves a grain that
/// changes from frame to frame.
const SAME: u8 = 8;
/// A delay is at least this, in hundredths of a second: browsers play shorter ones slowly.
const LEAST_DELAY: u32 = 2;

/// An animated GIF being written. Pixels come as four bytes each, blue, green, red and one
/// ignored, rows from the top.
pub struct Gif<W: Write> {
    out: W,
    width: usize,
    height: usize,
    /// What the picture shows so far.
    shown: Vec<u8>,
    /// The last frame, which waits to know how long it stays: its bytes and its delay.
    waiting: Option<(Vec<u8>, u32)>,
}

impl<W: Write> Gif<W> {
    pub fn new(mut out: W, width: u16, height: u16) -> io::Result<Self> {
        out.write_all(b"GIF89a")?;
        out.write_all(&width.to_le_bytes())?;
        out.write_all(&height.to_le_bytes())?;
        // No colours of the whole file, and the frames repeat for ever.
        out.write_all(&[0x70, 0, 0])?;
        out.write_all(&[0x21, 0xFF, 11])?;
        out.write_all(b"NETSCAPE2.0")?;
        out.write_all(&[3, 1, 0, 0, 0])?;
        let (width, height) = (width as usize, height as usize);
        Ok(Gif { out, width, height, shown: Vec::new(), waiting: None })
    }

    /// Adds a frame that stays for `delay` hundredths of a second.
    pub fn frame(&mut self, pixels: &[u8], delay: u32) -> io::Result<()> {
        debug_assert_eq!(pixels.len(), self.width * self.height * 4);
        let first = self.shown.is_empty();
        if first {
            self.shown = pixels.to_vec();
        }
        let Some(changed) = self.changed(pixels, first) else {
            if let Some((_, waited)) = &mut self.waiting {
                *waited += delay;
            }
            return Ok(());
        };
        let block = self.encode(pixels, changed, first);
        self.flush()?;
        self.waiting = Some((block, delay));
        Ok(())
    }

    /// Ends the file and gives back what it was written to.
    pub fn finish(mut self) -> io::Result<W> {
        self.flush()?;
        self.out.write_all(&[0x3B])?;
        self.out.flush()?;
        Ok(self.out)
    }

    /// Writes the frame that waits, with how long it stays.
    fn flush(&mut self) -> io::Result<()> {
        let Some((block, delay)) = self.waiting.take() else { return Ok(()) };
        let delay = delay.clamp(LEAST_DELAY, u16::MAX as u32) as u16;
        // What it covers stays when the next frame comes, and one index is transparent.
        self.out.write_all(&[0x21, 0xF9, 4, 0b0000_0101])?;
        self.out.write_all(&delay.to_le_bytes())?;
        self.out.write_all(&[TRANSPARENT, 0])?;
        self.out.write_all(&block)
    }

    /// The box of what has changed: left, top, right and bottom.
    fn changed(&self, pixels: &[u8], first: bool) -> Option<(usize, usize, usize, usize)> {
        if first {
            return Some((0, 0, self.width, self.height));
        }
        let stride = self.width * 4;
        let (mut left, mut top, mut right, mut bottom) = (self.width, self.height, 0, 0);
        for (y, (row, old)) in pixels.chunks_exact(stride).zip(self.shown.chunks_exact(stride)).enumerate() {
            let apart = |n: &usize| (0..3).any(|c| row[n * 4 + c].abs_diff(old[n * 4 + c]) > SAME);
            if let Some(start) = (0..self.width).find(|n| apart(n)) {
                let end = (0..self.width).rev().find(|n| apart(n)).unwrap_or(start);
                (left, right) = (left.min(start), right.max(end + 1));
                (top, bottom) = (top.min(y), y + 1);
            }
        }
        (right > left).then_some((left, top, right, bottom))
    }

    /// What is written of a frame but its delay: where it goes, its colours and its pixels.
    fn encode(&mut self, pixels: &[u8], (left, top, right, bottom): (usize, usize, usize, usize), first: bool) -> Vec<u8> {
        let (w, h) = (right - left, bottom - top);
        let stride = self.width * 4;
        let mut bins = vec![Bin::default(); 1 << 15];
        let mut changed = vec![false; w * h];
        for y in 0..h {
            let start = (top + y) * stride + left * 4;
            let (row, old) = (&pixels[start..start + w * 4], &self.shown[start..start + w * 4]);
            for x in 0..w {
                let p = &row[x * 4..x * 4 + 3];
                if first || (0..3).any(|c| p[c].abs_diff(old[x * 4 + c]) > SAME) {
                    changed[y * w + x] = true;
                    bins[bin(p)].add(p);
                }
            }
        }
        let (palette, map) = colours(&bins);
        let mut indices = Vec::with_capacity(w * h);
        for y in 0..h {
            let start = (top + y) * stride + left * 4;
            for x in 0..w {
                if changed[y * w + x] {
                    let at = start + x * 4;
                    indices.push(map[bin(&pixels[at..at + 3])]);
                    self.shown[at..at + 3].copy_from_slice(&pixels[at..at + 3]);
                } else {
                    indices.push(TRANSPARENT);
                }
            }
        }
        let mut block = Vec::with_capacity(w * h / 2);
        block.push(0x2C);
        for value in [left, top, w, h] {
            block.extend_from_slice(&(value as u16).to_le_bytes());
        }
        // A table of 256 colours of its own.
        block.push(0b1000_0111);
        for n in 0..256 {
            block.extend_from_slice(&palette.get(n).copied().unwrap_or([0; 3]));
        }
        lzw(&indices, &mut block);
        block
    }
}

/// The colours that fall into a 32nd of each channel.
#[derive(Clone, Copy, Default)]
struct Bin {
    count: u32,
    sum: [u32; 3],
}

impl Bin {
    fn add(&mut self, pixel: &[u8]) {
        self.count += 1;
        // In the order the file has them: red, green, blue.
        for (sum, c) in self.sum.iter_mut().zip([pixel[2], pixel[1], pixel[0]]) {
            *sum += c as u32;
        }
    }
}

fn bin(pixel: &[u8]) -> usize {
    (pixel[2] as usize >> 3) << 10 | (pixel[1] as usize >> 3) << 5 | pixel[0] as usize >> 3
}

/// The channel of a bin: red, green or blue.
fn channel(bin: usize, c: usize) -> usize {
    bin >> (10 - 5 * c) & 31
}

/// Bins that will have one colour, until they are cut in two.
struct Group {
    bins: Vec<usize>,
    /// The channel they are the most apart in.
    across: usize,
    /// How much cutting them is worth: how many pixels they have, by how far apart they are.
    weight: u64,
}

impl Group {
    fn new(list: Vec<usize>, bins: &[Bin]) -> Group {
        let spread = |c: usize| {
            let (mut low, mut high) = (usize::MAX, 0);
            for &n in &list {
                let value = channel(n, c);
                (low, high) = (low.min(value), high.max(value));
            }
            high.saturating_sub(low)
        };
        let widths = [spread(0), spread(1), spread(2)];
        let across = (0..3).max_by_key(|&c| widths[c]).unwrap_or(0);
        let count: u64 = list.iter().map(|&n| bins[n].count as u64).sum();
        Group { bins: list, across, weight: count * widths[across] as u64 }
    }
}

/// The colours of a frame, and the colour of every bin: groups of bins are cut in two across their
/// widest channel, at the middle of their pixels, the largest group first.
fn colours(bins: &[Bin]) -> (Vec<[u8; 3]>, Vec<u8>) {
    let used: Vec<usize> = (0..bins.len()).filter(|&n| bins[n].count > 0).collect();
    let mut groups = vec![Group::new(used, bins)];
    while groups.len() < COLOURS {
        let Some(largest) = (0..groups.len()).filter(|&n| groups[n].bins.len() > 1).max_by_key(|&n| groups[n].weight) else { break };
        let Group { bins: mut list, across, .. } = groups.swap_remove(largest);
        list.sort_unstable_by_key(|&n| channel(n, across));
        let half: u64 = list.iter().map(|&n| bins[n].count as u64).sum::<u64>() / 2;
        let mut passed = 0;
        let mut cut = 1;
        for (k, &n) in list.iter().enumerate() {
            passed += bins[n].count as u64;
            if passed >= half {
                cut = (k + 1).clamp(1, list.len() - 1);
                break;
            }
        }
        let rest = list.split_off(cut);
        groups.push(Group::new(list, bins));
        groups.push(Group::new(rest, bins));
    }
    let mut map = vec![0u8; bins.len()];
    let mut palette = Vec::with_capacity(groups.len());
    for group in &groups {
        let (mut count, mut sum) = (0u64, [0u64; 3]);
        for &n in &group.bins {
            count += bins[n].count as u64;
            for (sum, &part) in sum.iter_mut().zip(&bins[n].sum) {
                *sum += part as u64;
            }
            map[n] = palette.len() as u8;
        }
        palette.push(sum.map(|s| (s / count.max(1)) as u8));
    }
    (palette, map)
}

/// Packs the indices with LZW into blocks of 255 bytes at most.
fn lzw(indices: &[u8], out: &mut Vec<u8>) {
    const BITS: u32 = 8;
    const LIMIT: usize = 4096;
    const SLOTS: usize = 8191;
    let (clear, end) = (1u32 << BITS, (1u32 << BITS) + 1);
    out.push(BITS as u8);
    let mut table = vec![u32::MAX; SLOTS];
    let mut codes = vec![0u16; SLOTS];
    let (mut next, mut width) = (end + 1, BITS + 1);
    let mut sink = Blocks { out, bits: 0, count: 0, block: Vec::with_capacity(255) };
    sink.put(clear, width);
    let mut prefix = None;
    for &index in indices {
        let Some(current) = prefix else {
            prefix = Some(index as u32);
            continue;
        };
        let key = current << 8 | index as u32;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) % SLOTS;
        while table[slot] != u32::MAX && table[slot] != key {
            slot = (slot + 1) % SLOTS;
        }
        if table[slot] == key {
            prefix = Some(codes[slot] as u32);
            continue;
        }
        sink.put(current, width);
        prefix = Some(index as u32);
        if (next as usize) < LIMIT {
            table[slot] = key;
            codes[slot] = next as u16;
            // The reader grows its codes as it reads: one step behind this one.
            if next == 1 << width && width < 12 {
                width += 1;
            }
            next += 1;
        } else {
            sink.put(clear, width);
            table.fill(u32::MAX);
            next = end + 1;
            width = BITS + 1;
        }
    }
    if let Some(current) = prefix {
        sink.put(current, width);
    }
    sink.put(end, width);
    sink.finish();
}

/// Codes of any width packed into bytes, and the bytes into blocks.
struct Blocks<'a> {
    out: &'a mut Vec<u8>,
    bits: u64,
    count: u32,
    block: Vec<u8>,
}

impl Blocks<'_> {
    fn put(&mut self, code: u32, width: u32) {
        self.bits |= (code as u64) << self.count;
        self.count += width;
        while self.count >= 8 {
            self.block.push(self.bits as u8);
            self.bits >>= 8;
            self.count -= 8;
            if self.block.len() == 255 {
                self.write();
            }
        }
    }

    fn write(&mut self) {
        self.out.push(self.block.len() as u8);
        self.out.append(&mut self.block);
    }

    fn finish(mut self) {
        if self.count > 0 {
            self.block.push(self.bits as u8);
        }
        if !self.block.is_empty() {
            self.write();
        }
        self.out.push(0);
    }
}

/// A picture made smaller, each new pixel the mean of those it covers: four bytes a pixel.
pub fn smaller(pixels: &[u8], width: usize, height: usize, new_width: usize, new_height: usize) -> Vec<u8> {
    let mut out = vec![0u8; new_width * new_height * 4];
    for y in 0..new_height {
        let (top, bottom) = (y * height / new_height, ((y + 1) * height / new_height).max(y * height / new_height + 1));
        for x in 0..new_width {
            let (left, right) = (x * width / new_width, ((x + 1) * width / new_width).max(x * width / new_width + 1));
            let mut sum = [0u32; 3];
            for row in top..bottom {
                let start = row * width * 4;
                for p in pixels[start + left * 4..start + right * 4].chunks_exact(4) {
                    for c in 0..3 {
                        sum[c] += p[c] as u32;
                    }
                }
            }
            let count = ((bottom - top) * (right - left)) as u32;
            let at = (y * new_width + x) * 4;
            for c in 0..3 {
                out[at + c] = (sum[c] / count) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of one colour with a square of another at a place.
    fn frame(width: usize, height: usize, at: usize) -> Vec<u8> {
        let mut pixels = vec![200u8; width * height * 4];
        for y in 10..20 {
            for x in at..at + 10 {
                pixels[(y * width + x) * 4..(y * width + x) * 4 + 3].copy_from_slice(&[30, 60, 240]);
            }
        }
        pixels
    }

    #[test]
    fn a_frame_keeps_only_what_has_changed() {
        let mut bytes = Vec::new();
        let mut gif = Gif::new(&mut bytes, 64, 32).unwrap();
        gif.frame(&frame(64, 32, 4), 10).unwrap();
        // Nothing changed: the frame before stays longer.
        gif.frame(&frame(64, 32, 4), 10).unwrap();
        gif.frame(&frame(64, 32, 30), 10).unwrap();
        gif.finish().unwrap();
        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(bytes.last(), Some(&0x3B));
        let delays: Vec<u16> = bytes
            .windows(8)
            .filter(|w| w[0] == 0x21 && w[1] == 0xF9 && w[2] == 4)
            .map(|w| u16::from_le_bytes([w[4], w[5]]))
            .collect();
        assert_eq!(delays, [20, 10]);
    }

    #[test]
    fn a_picture_made_smaller_keeps_its_colours() {
        let pixels = frame(64, 32, 4);
        let small = smaller(&pixels, 64, 32, 32, 16);
        assert_eq!(small.len(), 32 * 16 * 4);
        assert_eq!(&small[..3], &[200, 200, 200]);
        assert_eq!(&small[(7 * 32 + 5) * 4..(7 * 32 + 5) * 4 + 3], &[30, 60, 240]);
    }

    #[test]
    fn many_colours_are_cut_to_a_table() {
        let mut bins = vec![Bin::default(); 1 << 15];
        for n in 0..4000usize {
            let p = [(n % 251) as u8, (n % 241) as u8, (n % 239) as u8];
            bins[bin(&p)].add(&p);
        }
        let (palette, map) = colours(&bins);
        assert!(palette.len() <= COLOURS && palette.len() > 100, "{}", palette.len());
        assert!(map.iter().all(|&n| (n as usize) < palette.len()));
    }

    /// A frame of many colours packs past the table of codes, and a square goes across it: the
    /// file is left for another program to read, if asked for.
    #[test]
    fn a_long_frame_packs_past_the_table_of_codes() {
        let (w, h) = (300usize, 200usize);
        let mut pixels = vec![0u8; w * h * 4];
        for (n, p) in pixels.chunks_exact_mut(4).enumerate() {
            p[0] = (n * 7 % 251) as u8;
            p[1] = (n * 13 % 241) as u8;
            p[2] = (n * 29 % 239) as u8;
        }
        let mut bytes = Vec::new();
        let mut gif = Gif::new(&mut bytes, w as u16, h as u16).unwrap();
        gif.frame(&pixels, 7).unwrap();
        for n in 0..8 {
            let mut moved = pixels.clone();
            for y in 40..80 {
                for x in 20 + n * 25..60 + n * 25 {
                    moved[(y * w + x) * 4..(y * w + x) * 4 + 3].copy_from_slice(&[250, 250, 250]);
                }
            }
            gif.frame(&moved, 7).unwrap();
        }
        gif.finish().unwrap();
        if let Some(folder) = std::env::var_os("SCREENSHOT_PREVIEW") {
            std::fs::write(std::path::Path::new(&folder).join("many-colours.gif"), &bytes).unwrap();
        }
        assert!(bytes.len() > 1000);
    }
}
