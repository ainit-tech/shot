//! Areas of a window: the parts that a screenshot is most often taken of, a panel, a table, a
//! message, a card, a picture or a paragraph, found in the pixels of the frozen screen by filling.
//! The surface under the mouse is filled with its colour. Thin long bars on it, lines and scroll
//! bars, are its walls; what it and they wholly surround, text, icons, pictures, cards, is its
//! furniture; and where it narrows to a passage much narrower than the space on either side, it is
//! two rooms with a door between them. The area under the mouse is the block of furniture under
//! it, if that is worth a screenshot, or else the room; on a band across the top or the bottom of
//! the window, its frame, there is no area. A surface that goes on smoothly past its colour, a
//! gradient or a wallpaper of little drawings, is filled on the picture smoothed of them. Tables,
//! found by the rhythm of their rows, and grids, found by their cells, light up whole, whatever
//! the colours of the cells.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::geometry::{Point, Rect};
use crate::image::Image;

/// How much neighbouring pixels may differ in a colour channel and still be one smooth surface.
const SAME: u8 = 6;
/// How far from the colour of a surface a pixel may be in a channel and still be of it.
const TOLERANCE: f32 = 12.0;
/// A pixel differs from a neighbour along its row.
const ACROSS: u8 = 1;
/// A pixel differs from a neighbour along its column.
const DOWN: u8 = 2;
/// A wall runs at least this far and is no thicker than this, at the scale of 100%, as the other
/// lengths here.
const WALL_LONG: f32 = 48.0;
const WALL_THIN: f32 = 12.0;
/// Two rooms part where the way between them narrows by this share of the narrower one's widest
/// place, and by this much at least.
const DOOR: f32 = 0.25;
const LEAST_DOOR: f32 = 8.0;
/// The smallest room worth a screenshot, the height of a message of one line on a wallpaper,
/// which is one too, and the smallest block of furniture worth one: two lines of text are not.
const PANEL: (f32, f32) = (48.0, 40.0);
const MESSAGE: f32 = 26.0;
const BLOCK: (f32, f32) = (64.0, 56.0);
/// Furniture this close together is one block: letters a word, lines a paragraph.
const JOIN: f32 = 6.0;
/// Bands at the top or the bottom of a window thinner than this, and across most of it, are its
/// frame. A cut across rooms is made no further than this from their ends.
const FRAME: f32 = 160.0;
const ACROSS_MOST: f32 = 0.6;
/// How much of the height of the window its frame takes up at the top at most, and at the bottom.
const FRAME_TOP: f32 = 0.25;
const FRAME_BOTTOM: f32 = 0.15;
/// An area this much of the window is the window.
const MOST: f32 = 0.9;
/// How far apart neighbours on the smoothed picture may be and be of one surface, and how far
/// around a pixel it is smoothed from.
const STEP: u8 = 3;
const SMOOTHING: f32 = 4.0;
/// A surface this much of whose border is smooth goes on past its colour: a gradient, if the
/// smoothed picture fills on from it to `1 / GRADIENT` of its size at least.
const SOFT: f32 = 0.05;
const GRADIENT: f32 = 0.5;
/// A surface is filled from a pixel of a part of it this large at least, in pixels at 100%: not
/// from the inside of a letter.
const SEED_LEAST: f32 = 64.0;
/// A table this much of the width of its rows' background wide is as wide as it.
const MOST_WIDE: f32 = 0.75;
/// A lit row across a panel, a selected item of a list, is between these tall.
const LIT_ROW: (f32, f32) = (12.0, 80.0);
/// The smallest cell of a grid.
const CELL: (f32, f32) = (16.0, 12.0);
/// A block no more of whose box than this is furniture is sparse, as text is.
const SPARSE: f32 = 0.3;
/// What is around a box is busy when this much of its neighbouring pixels are apart.
const BUSY: f32 = 0.015;
/// How thick a line between things is, and how far things may be off one another by.
const LINE: f32 = 2.0;
const SLACK: f32 = 4.0;
/// Pieces of furniture no larger than this are letters and icons.
const LETTER: f32 = 40.0;
/// How many bytes of the surfaces of the colours of a window are kept, and how many windows.
const TONES: usize = 256 << 20;
const WINDOWS: usize = 4;

type Color = [f32; 3];
/// How many parts of a pixel distances are in.
const UNIT: u32 = 256;
/// The eighth of its value that a colour is in, in each channel: what surfaces are found by.
type Tone = [u8; 3];
/// A surface: its colour's tone and its label among the surfaces of that tone.
type Surface = (Tone, u32);

/// The areas of the windows of a frozen screen, found as the mouse comes to them.
pub struct Areas {
    width: i32,
    height: i32,
    /// The windows filled so far, the last one the mouse was on first.
    windows: VecDeque<(Rect, u32, Filling)>,
}

impl Areas {
    pub fn new(image: &Image) -> Self {
        Areas { width: image.width as i32, height: image.height as i32, windows: VecDeque::new() }
    }

    /// The area of the window under the point, at the monitor's scale. Nothing if no part of the
    /// window is one, but the window itself.
    pub fn at(&mut self, image: &Image, window: Rect, p: Point, scale: f32) -> Option<Rect> {
        let window = window.intersect(Rect::new(0, 0, self.width, self.height));
        if window.is_empty() || !window.contains(p) {
            return None;
        }
        let known = self.windows.iter().position(|(rect, bits, _)| *rect == window && *bits == scale.to_bits());
        let filling = match known {
            Some(n) => self.windows.remove(n).unwrap(),
            None => (window, scale.to_bits(), Filling::new(image, window, scale)),
        };
        self.windows.push_front(filling);
        self.windows.truncate(WINDOWS);
        let area = self.windows[0].2.area_at(p.x - window.left, p.y - window.top)?;
        Some(area.offset(window.left, window.top))
    }
}

/// The rooms of a surface, over the box of the window that holds them.
struct Rooms {
    crop: Rect,
    /// The room of every pixel of the box, 0 for none.
    labels: Vec<u32>,
    furniture: Vec<bool>,
    /// The box of every room, in the window.
    boxes: Vec<Rect>,
    furnished: HashMap<u32, Rc<Furnished>>,
}

impl Rooms {
    /// The room at a point of the window, 0 for none.
    fn at(&self, x: i32, y: i32) -> u32 {
        if !self.crop.contains(Point::new(x, y)) {
            return 0;
        }
        self.labels[(y - self.crop.top) as usize * self.crop.width() as usize + (x - self.crop.left) as usize]
    }
}

/// A room and the blocks of furniture in it.
struct Furnished {
    room: Rect,
    /// The block of every pixel of the room, 0 for none.
    blocks: Vec<u32>,
    /// The box of every block, and whether it is sparse, as text is.
    boxes: HashMap<u32, (Rect, bool)>,
}

/// A line of text: the height of its middle and its box.
#[derive(Clone, Copy)]
struct Line {
    middle: f32,
    rect: Rect,
}

/// A window of the frozen screen and all found in it so far, in its own coordinates.
struct Filling {
    w: i32,
    h: i32,
    scale: f32,
    pixels: Vec<[u8; 3]>,
    /// For every pixel, whether it differs from its neighbours along its row and its column.
    marks: Vec<u8>,
    /// The picture smoothed of small drawings, letters and grain, for gradients; the surface on it
    /// that every pixel is filled to so far, and the size and the box of each.
    smooth: Vec<[u8; 3]>,
    chains: Vec<u32>,
    chain_extents: Vec<(u32, Rect)>,
    tones: HashMap<Tone, Rc<Parts>>,
    /// The tones in the order they were found, the oldest to go first.
    tone_order: VecDeque<Tone>,
    softness: HashMap<Surface, bool>,
    gradients: HashMap<Surface, bool>,
    grids: HashMap<Surface, Option<Rect>>,
    /// Which of `rooms` every surface and every chain is in.
    known: HashMap<Surface, usize>,
    chained: HashMap<u32, usize>,
    rooms: Vec<Rooms>,
    tables: Option<Vec<Rect>>,
}

impl Filling {
    fn new(image: &Image, window: Rect, scale: f32) -> Self {
        let (w, h) = (window.width() as usize, window.height() as usize);
        let mut pixels = Vec::with_capacity(w * h);
        for y in window.top as usize..window.bottom as usize {
            let start = (y * image.width as usize + window.left as usize) * 4;
            pixels.extend(image.pixels[start..start + w * 4].chunks_exact(4).map(|p| [p[0], p[1], p[2]]));
        }
        let mut marks = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if x + 1 < w && !alike(pixels[i], pixels[i + 1], SAME) {
                    marks[i] |= ACROSS;
                    marks[i + 1] |= ACROSS;
                }
                if y + 1 < h && !alike(pixels[i], pixels[i + w], SAME) {
                    marks[i] |= DOWN;
                    marks[i + w] |= DOWN;
                }
            }
        }
        Filling {
            w: w as i32,
            h: h as i32,
            scale,
            pixels,
            marks,
            smooth: Vec::new(),
            chains: Vec::new(),
            chain_extents: Vec::new(),
            tones: HashMap::new(),
            tone_order: VecDeque::new(),
            softness: HashMap::new(),
            gradients: HashMap::new(),
            grids: HashMap::new(),
            known: HashMap::new(),
            chained: HashMap::new(),
            rooms: Vec::new(),
            tables: None,
        }
    }

    /// A length at the scale of 100% at the monitor's scale.
    fn px(&self, length: f32) -> i32 {
        (length * self.scale).round() as i32
    }

    fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    fn index(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    /// What lights up under the point: a box, or nothing for the whole window.
    fn area_at(&mut self, x: i32, y: i32) -> Option<Rect> {
        if self.tables.is_none() {
            self.tables = Some(self.find_tables());
        }
        // A table lights up whole, whatever the colours of its cells.
        let p = Point::new(x, y);
        if let Some(table) = self.tables.iter().flatten().find(|table| table.contains(p)) {
            return Some(*table);
        }
        let (room, block) = self.room_at(x, y)?;
        // A point on a thin outgrowth of a room, as an address bar on the surface of the panel
        // under it, is on the window's frame rather than in the room.
        if !room.contains(p) {
            return None;
        }
        // The window's frame: thin bands across its top or its bottom, a title bar, toolbars, an
        // address bar, a status bar.
        let band = room.height() < self.px(FRAME) && room.width() as f32 >= ACROSS_MOST * self.w as f32;
        if band && (room.bottom as f32 <= FRAME_TOP * self.h as f32 || room.top as f32 >= (1.0 - FRAME_BOTTOM) * self.h as f32) {
            return None;
        }
        let mut area = room;
        if let Some((block, sparse)) = block
            && self.big(block, BLOCK)
            && size(block) <= 0.8 * size(room)
            && !(sparse && (size(block) >= 0.5 * size(room) || size(room) <= 0.4 * size(self.bounds())))
        {
            area = block;
        }
        (size(area) < MOST * size(self.bounds())).then(|| self.bordered(area))
    }

    fn big(&self, rect: Rect, least: (f32, f32)) -> bool {
        rect.width() >= self.px(least.0) && rect.height() >= self.px(least.1)
    }

    /// The room under the point, worth a screenshot, and the block of furniture under the point
    /// with whether it is sparse, if there is one.
    fn room_at(&mut self, x: i32, y: i32) -> Option<(Rect, Option<(Rect, bool)>)> {
        let (mut px, mut py) = (x, y);
        for _ in 0..4 {
            let (colour, seed) = self.surface_at(px, py)?;
            let surfaces = self.surfaces_of(colour);
            let label = surfaces.labels[self.index(seed.0, seed.1)];
            if label == 0 {
                (px, py) = self.around(Rect::new(seed.0, seed.1, seed.0 + 1, seed.1 + 1), x, y);
                continue;
            }
            let rooms = self.rooms(colour, seed);
            let here = self.rooms[rooms].at(x, y);
            let room_id = if here != 0 { here } else { self.rooms[rooms].at(seed.0, seed.1) };
            if room_id == 0 {
                // A surface too thin for a room, a line, a narrow strip: the surface around it.
                (px, py) = self.around(surfaces.boxes[label as usize], x, y);
                continue;
            }
            // A cell of a grid: the grid, whatever stands in its cells.
            if let Some(grid) = self.lattice(colour, label) {
                return Some((grid, None));
            }
            let furnished = self.furnished(rooms, room_id);
            let room = furnished.room;
            if self.big(room, PANEL) || (self.big(room, (PANEL.0, MESSAGE)) && self.busy_around(room)) {
                let block = room.contains(Point::new(x, y)).then(|| {
                    let at = (y - room.top) as usize * room.width() as usize + (x - room.left) as usize;
                    furnished.boxes.get(&furnished.blocks[at]).copied()
                });
                return Some((room, block.flatten()));
            }
            // Too small a room, a button, a field, a lit row: the surface around it.
            (px, py) = self.around(room, x, y);
        }
        None
    }

    /// A point of the surface around a box that the point is on too, as the panel a lit row or a
    /// field stands in: above the box, below it, to its left or its right.
    fn around(&mut self, rect: Rect, x: i32, y: i32) -> (i32, i32) {
        let (mx, my) = ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2);
        let out = self.px(3.0);
        let points = [(mx, rect.top - out), (mx, rect.bottom + out - 1), (rect.left - out, my), (rect.right + out - 1, my)];
        let inside = points.into_iter().filter(|&(cx, cy)| self.bounds().contains(Point::new(cx, cy)));
        let points: Vec<(i32, i32)> = inside.collect();
        for &(cx, cy) in &points {
            if let Some((colour, seed)) = self.surface_at(cx, cy)
                && self.surfaces_of(colour).labels[self.index(seed.0, seed.1)] != 0
            {
                let rooms = self.rooms(colour, seed);
                if self.rooms[rooms].at(x, y) != 0 {
                    return (cx, cy);
                }
            }
        }
        // A stripe of a list across it: the surface over it, which the list is a room of.
        points.first().copied().unwrap_or(((rect.left - out).max(0), (rect.top - out).max(0)))
    }

    /// The surfaces of a colour's tone.
    fn surfaces_of(&mut self, colour: Color) -> Rc<Parts> {
        let tone = tone_of(colour);
        if let Some(surfaces) = self.tones.get(&tone) {
            return surfaces.clone();
        }
        // Around the middle of the tone, whichever colour of it comes first.
        let near = tone.map(|c| std::array::from_fn::<bool, 256, _>(|value| (value as u8).abs_diff(c * 8 + 4) <= TOLERANCE as u8));
        let mask: Vec<bool> = self.pixels.iter().map(|p| near[0][p[0] as usize] & near[1][p[1] as usize] & near[2][p[2] as usize]).collect();
        let surfaces = Rc::new(parts(&mask, self.w as usize, false));
        let kept = (TONES / (4 * self.pixels.len()).max(1)).max(4);
        while self.tone_order.len() >= kept {
            if let Some(oldest) = self.tone_order.pop_front() {
                self.tones.remove(&oldest);
            }
        }
        self.tone_order.push_back(tone);
        self.tones.insert(tone, surfaces.clone());
        surfaces
    }

    /// The colour of most of the smooth pixels of a box, by colours a little apart, and how many
    /// smooth pixels there are.
    fn common(&self, rect: Rect) -> (Option<Color>, usize) {
        let mut keyed: Vec<(u16, [u8; 3])> = Vec::new();
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let i = self.index(x, y);
                if self.marks[i] == 0 {
                    let [a, b, c] = self.pixels[i].map(|v| v as u16 / 8);
                    keyed.push((a * 1024 + b * 32 + c, self.pixels[i]));
                }
            }
        }
        keyed.sort_unstable_by_key(|(key, _)| *key);
        let (mut best, mut run_start, mut best_run) = (0, 0, (0, 0));
        for n in 1..=keyed.len() {
            if n == keyed.len() || keyed[n].0 != keyed[run_start].0 {
                if n - run_start > best {
                    best = n - run_start;
                    best_run = (run_start, n);
                }
                run_start = n;
            }
        }
        if best == 0 {
            return (None, 0);
        }
        let mut sum = [0f32; 3];
        for (_, pixel) in &keyed[best_run.0..best_run.1] {
            for c in 0..3 {
                sum[c] += pixel[c] as f32;
            }
        }
        (Some(sum.map(|s| s / best as f32)), keyed.len())
    }

    /// The colour of the smooth surface nearest the point, and the box around the point where it
    /// is found.
    fn colour_at(&self, x: i32, y: i32) -> Option<(Color, Rect)> {
        [3.0, 8.0, 16.0, 32.0, 64.0, 128.0].into_iter().find_map(|radius| {
            let r = self.px(radius);
            let rect = Rect::new(x - r, y - r, x + r + 1, y + r + 1).intersect(self.bounds());
            let (colour, count) = self.common(rect);
            colour.filter(|_| count >= 6).map(|colour| (colour, rect))
        })
    }

    /// The colour of the smooth surface nearest the point, and a pixel of it to fill from.
    fn surface_at(&mut self, x: i32, y: i32) -> Option<(Color, (i32, i32))> {
        let (colour, rect) = self.colour_at(x, y)?;
        // Not the inside of a letter, an o or a 0, of the colour of the surface around it.
        let surfaces = self.surfaces_of(colour);
        let least = (SEED_LEAST * self.scale * self.scale) as u32;
        let (mut nearest, mut whole) = (None, None);
        for py in rect.top..rect.bottom {
            for px in rect.left..rect.right {
                let i = self.index(px, py);
                if self.marks[i] != 0 || !of_colour(self.pixels[i], colour) {
                    continue;
                }
                let distance = (px - x) * (px - x) + (py - y) * (py - y);
                if nearest.is_none_or(|(d, _)| distance < d) {
                    nearest = Some((distance, (px, py)));
                }
                let label = surfaces.labels[i];
                if label > 0 && surfaces.sizes[label as usize] >= least && whole.is_none_or(|(d, _)| distance < d) {
                    whole = Some((distance, (px, py)));
                }
            }
        }
        whole.or(nearest).map(|(_, seed)| (colour, seed))
    }

    /// Whether the surface around the seed goes on smoothly past its colour, as a gradient, a
    /// wallpaper, a picture's grain do: much of its border is smooth.
    fn soft(&mut self, colour: Color, seed: (i32, i32)) -> bool {
        let surfaces = self.surfaces_of(colour);
        let label = surfaces.labels[self.index(seed.0, seed.1)];
        if label == 0 {
            return false;
        }
        let key = (tone_of(colour), label);
        if let Some(&soft) = self.softness.get(&key) {
            return soft;
        }
        let rect = surfaces.boxes[label as usize].inflate(1).intersect(self.bounds());
        let (mut ring, mut smooth) = (0u32, 0u32);
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let i = self.index(x, y);
                if surfaces.labels[i] == label {
                    continue;
                }
                let beside = (-1..=1).any(|dy| {
                    (-1..=1).any(|dx| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx >= 0 && ny >= 0 && nx < self.w && ny < self.h && surfaces.labels[self.index(nx, ny)] == label
                    })
                });
                if beside {
                    ring += 1;
                    smooth += (self.marks[i] == 0) as u32;
                }
            }
        }
        let soft = smooth as f32 >= SOFT * ring.max(1) as f32;
        self.softness.insert(key, soft);
        soft
    }

    /// Whether the surface around the seed is a part of a gradient, a wallpaper, a picture's
    /// grain: its border smooth, and the smoothed picture filling on from it far past its colour,
    /// not by the blurred edges of a scaled picture only.
    fn gradient(&mut self, colour: Color, seed: (i32, i32)) -> bool {
        let surfaces = self.surfaces_of(colour);
        let label = surfaces.labels[self.index(seed.0, seed.1)];
        let key = (tone_of(colour), label);
        if let Some(&gradient) = self.gradients.get(&key) {
            return gradient;
        }
        let gradient = self.soft(colour, seed) && {
            let chain = self.chain(seed);
            surfaces.sizes[label as usize] as f32 <= GRADIENT * self.chain_extents[chain].0 as f32
        };
        self.gradients.insert(key, gradient);
        gradient
    }

    /// The surface around the seed on the smoothed picture, filled from neighbour to neighbour: a
    /// wallpaper of drawings on a gradient is one surface. Its number in `chain_extents`.
    fn chain(&mut self, seed: (i32, i32)) -> usize {
        if self.smooth.is_empty() {
            self.smooth = smoothed(&self.pixels, self.w as usize, self.px(SMOOTHING).max(1) as usize);
            self.chains = vec![0; self.pixels.len()];
            self.chain_extents = vec![(0, Rect::default())];
        }
        let start = self.index(seed.0, seed.1);
        if self.chains[start] != 0 {
            return self.chains[start] as usize;
        }
        let id = self.chain_extents.len() as u32;
        let width = self.w as usize;
        let (mut size, mut extent) = (0u32, Rect::new(seed.0, seed.1, seed.0 + 1, seed.1 + 1));
        let mut stack = vec![start];
        self.chains[start] = id;
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = ((i % width) as i32, (i / width) as i32);
            extent = extent.union(Rect::new(x, y, x + 1, y + 1));
            let mut visit = |n: usize| {
                if self.chains[n] == 0 && alike(self.smooth[i], self.smooth[n], STEP) {
                    self.chains[n] = id;
                    stack.push(n);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < self.w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - width);
            }
            if y + 1 < self.h {
                visit(i + width);
            }
        }
        self.chain_extents.push((size, extent));
        id as usize
    }

    /// The surfaces of a panel's colour that lit rows across it part, a selected item of a list:
    /// one above and one below a band of another colour, as wide as they are and no taller than an
    /// item of two lines, are one surface with the band. The labels joined, and the bands.
    fn lit_rows(&self, surfaces: &Parts, label: u32) -> (Vec<u32>, Vec<Rect>) {
        let least = (SEED_LEAST * self.scale * self.scale) as u32;
        let slack = self.px(SLACK);
        let (low, high) = (self.px(LIT_ROW.0), self.px(LIT_ROW.1));
        let (mut joined, mut bands, mut todo) = (vec![label], Vec::new(), vec![label]);
        while let Some(n) = todo.pop() {
            let rect = surfaces.boxes[n as usize];
            for (m, other) in surfaces.boxes.iter().enumerate().skip(1) {
                let m = m as u32;
                if (other.left - rect.left).abs() > slack
                    || (other.right - rect.right).abs() > slack
                    || surfaces.sizes[m as usize] < least
                    || joined.contains(&m)
                {
                    continue;
                }
                let band = if (low..=high).contains(&(other.top - rect.bottom)) {
                    Rect::new(rect.left, rect.bottom, rect.right, other.top)
                } else if (low..=high).contains(&(rect.top - other.bottom)) {
                    Rect::new(rect.left, other.bottom, rect.right, rect.top)
                } else {
                    continue;
                };
                if self.lit(band) {
                    joined.push(m);
                    todo.push(m);
                    bands.push(band);
                }
            }
        }
        (joined, bands)
    }

    /// Whether a band is a lit row: most of it of one colour, the light of the row, whatever
    /// stands on it.
    fn lit(&self, band: Rect) -> bool {
        let Some(colour) = self.common(band).0 else { return false };
        let mut near = 0;
        for y in band.top..band.bottom {
            for x in band.left..band.right {
                near += of_colour(self.pixels[self.index(x, y)], colour) as usize;
            }
        }
        2 * near >= band.width() as usize * band.height() as usize
    }

    /// The lit rows across a surface that goes round them, if at all, by a thin margin: bands
    /// where the surface all but stops across its whole width, between rows where it spans it.
    /// The surface is `own` over `crop`, within the box `rect`.
    fn lit_bands(&self, own: &[bool], crop: Rect, rect: Rect) -> Vec<Rect> {
        let width = crop.width() as usize;
        let share: Vec<f32> = (rect.top..rect.bottom)
            .map(|y| {
                let start = (y - crop.top) as usize * width + (rect.left - crop.left) as usize;
                let row = &own[start..start + rect.width() as usize];
                row.iter().filter(|o| **o).count() as f32 / row.len() as f32
            })
            .collect();
        let (low, high) = (self.px(LIT_ROW.0) as usize, self.px(LIT_ROW.1) as usize);
        let mut bands = Vec::new();
        let mut i = 1;
        while i + 1 < share.len() {
            if share[i] >= 0.05 {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < share.len() && share[j] < 0.05 {
                j += 1;
            }
            if (low..=high).contains(&(j - i)) && j < share.len() && share[i - 1] >= 0.9 && share[j] >= 0.9 {
                let band = Rect::new(rect.left, rect.top + i as i32, rect.right, rect.top + j as i32);
                if self.lit(band) {
                    bands.push(band);
                }
            }
            i = j;
        }
        bands
    }

    /// The grid a surface is a cell of, as an hour of a calendar's week, a cell of a sheet:
    /// surfaces of one colour parted by thin lines, as high as the cells beside them in a row and
    /// as wide as those over and under them in a column, in three rows and two columns at least:
    /// not the headings of a table's columns over their bodies. The box of the grid, if so.
    fn lattice(&mut self, colour: Color, label: u32) -> Option<Rect> {
        let tone = tone_of(colour);
        if let Some(&grid) = self.grids.get(&(tone, label)) {
            return grid;
        }
        self.grids.insert((tone, label), None);
        let surfaces = self.surfaces_of(colour);
        let boxes = &surfaces.boxes;
        let least = (self.px(CELL.0), self.px(CELL.1));
        let large = |rect: &Rect| rect.width() >= least.0 && rect.height() >= least.1;
        if !large(&boxes[label as usize]) {
            return None;
        }
        let (even, apart) = (self.px(LINE + 1.0), self.px(SLACK));
        let (mut cells, mut todo) = (vec![label], vec![label]);
        while let Some(n) = todo.pop() {
            let cell = boxes[n as usize];
            for (m, other) in boxes.iter().enumerate().skip(1) {
                let row = (other.top - cell.top).abs() <= even && (other.bottom - cell.bottom).abs() <= even;
                let column = (other.left - cell.left).abs() <= even && (other.right - cell.right).abs() <= even;
                let beside = (0..=apart).contains(&(other.left - cell.right)) || (0..=apart).contains(&(cell.left - other.right));
                let over = (0..=apart).contains(&(other.top - cell.bottom)) || (0..=apart).contains(&(cell.top - other.bottom));
                if ((row && beside) || (column && over)) && large(other) && !cells.contains(&(m as u32)) {
                    cells.push(m as u32);
                    todo.push(m as u32);
                }
            }
        }
        let distinct = |values: &mut Vec<i32>| {
            values.sort_unstable();
            values.dedup();
            values.len()
        };
        let mut columns: Vec<i32> = cells.iter().map(|&n| boxes[n as usize].left / 4).collect();
        let mut rows: Vec<i32> = cells.iter().map(|&n| boxes[n as usize].top / 4).collect();
        if cells.len() < 6 || distinct(&mut columns) < 2 || distinct(&mut rows) < 3 {
            return None;
        }
        let grid = cells.iter().fold(Rect::default(), |grid, &n| grid.union(boxes[n as usize]));
        for &n in &cells {
            self.grids.insert((tone, n), Some(grid));
        }
        Some(grid)
    }

    /// The rooms of the surface around the seed: their number in `rooms`. The seed is on a surface
    /// of the colour.
    fn rooms(&mut self, colour: Color, seed: (i32, i32)) -> usize {
        let tone = tone_of(colour);
        let surfaces = self.surfaces_of(colour);
        let label = surfaces.labels[self.index(seed.0, seed.1)];
        if let Some(&known) = self.known.get(&(tone, label)) {
            return known;
        }
        let (joined, lit) = self.lit_rows(&surfaces, label);
        let mut rect = joined.iter().fold(Rect::default(), |rect, &n| rect.union(surfaces.boxes[n as usize]));
        let mut own = vec![false; rect.width() as usize * rect.height() as usize];
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let n = surfaces.labels[self.index(x, y)];
                own[(y - rect.top) as usize * rect.width() as usize + (x - rect.left) as usize] = n == label || joined.contains(&n);
            }
        }
        // With the thin lines along a lit row's edges, the lines between the items of the list.
        let line = self.px(LINE);
        for band in &lit {
            let band = Rect::new(band.left, band.top - line, band.right, band.bottom + line).intersect(rect);
            for y in band.top..band.bottom {
                let start = (y - rect.top) as usize * rect.width() as usize + (band.left - rect.left) as usize;
                own[start..start + band.width() as usize].fill(true);
            }
        }
        // A surface that goes on smoothly past its colour, a gradient, a wallpaper, a picture's
        // grain, is taken as the smoothed picture shows it. A surface that ends at edges all
        // round, a panel, a message, a field, however close in colour to what lies past them, is
        // taken as it is.
        let mut chained = None;
        if self.soft(colour, seed) {
            let chain = self.chain(seed);
            let (size, extent) = self.chain_extents[chain];
            if own.iter().filter(|o| **o).count() as f32 <= GRADIENT * size as f32 {
                if let Some(&known) = self.chained.get(&(chain as u32)) {
                    self.known.insert((tone, label), known);
                    return known;
                }
                chained = Some(chain as u32);
                rect = extent;
                own = vec![false; rect.width() as usize * rect.height() as usize];
                for y in rect.top..rect.bottom {
                    for x in rect.left..rect.right {
                        let i = self.index(x, y);
                        own[(y - rect.top) as usize * rect.width() as usize + (x - rect.left) as usize] = self.chains[i] == chain as u32;
                    }
                }
            }
        }
        self.rooms.push(self.divide(&own, rect));
        let number = self.rooms.len() - 1;
        for n in joined {
            self.known.insert((tone, n), number);
        }
        if let Some(chain) = chained {
            self.chained.insert(chain, number);
        }
        number
    }

    /// The rooms of a surface, `own` over the box `rect`.
    fn divide(&self, own: &[bool], rect: Rect) -> Rooms {
        // What is around the surface, far enough to see the walls along it whole, with the tables
        // that stand on it.
        let stands = |table: &Rect| {
            let common = table.intersect(rect);
            !common.is_empty() && (common.top..common.bottom).any(|y| {
                let start = (y - rect.top) as usize * rect.width() as usize + (common.left - rect.left) as usize;
                own[start..start + common.width() as usize].iter().any(|o| *o)
            })
        };
        let tables: Vec<Rect> = self.tables.iter().flatten().copied().filter(stands).collect();
        let reach = rect.inflate(self.px(WALL_LONG)).intersect(self.bounds());
        let crop = tables.iter().fold(reach, |crop, table| crop.union(*table));
        let (cw, ch) = (crop.width() as usize, crop.height() as usize);
        let mut inside = vec![false; cw * ch];
        for y in rect.top..rect.bottom {
            let from = (y - rect.top) as usize * rect.width() as usize;
            let to = (y - crop.top) as usize * cw + (rect.left - crop.left) as usize;
            inside[to..to + rect.width() as usize].copy_from_slice(&own[from..from + rect.width() as usize]);
        }
        let own = inside;
        // A wall is a bar alike all along: a line, a scroll bar; not a line of text.
        let bars = |bit: u8| -> Vec<bool> {
            let mut bars = Vec::with_capacity(cw * ch);
            for (y, row) in own.chunks_exact(cw).enumerate() {
                let start = self.index(crop.left, crop.top + y as i32);
                bars.extend(row.iter().zip(&self.marks[start..start + cw]).map(|(own, mark)| !own && mark & bit == 0));
            }
            bars
        };
        let long = self.px(WALL_LONG) as usize;
        let thick = self.px(WALL_THIN) as usize + 1;
        let down = runs(&bars(DOWN), cw, long, false);
        let across = runs(&bars(ACROSS), cw, long, true);
        let other: Vec<bool> = own.iter().map(|o| !o).collect();
        let wide = runs(&other, cw, thick, true);
        let tall = runs(&other, cw, thick, false);
        let mut open = vec![false; cw * ch];
        for i in 0..cw * ch {
            let wall = (down[i] && !wide[i]) || (across[i] && !tall[i]);
            open[i] = !own[i] && !wall;
        }
        // What the surface and its walls wholly surround is furniture.
        let holes = parts(&open, cw, false);
        let outer: Vec<bool> =
            holes.boxes.iter().map(|hole| hole.left == 0 || hole.top == 0 || hole.right == cw as i32 || hole.bottom == ch as i32).collect();
        let mut furniture: Vec<bool> = holes.labels.iter().map(|&hole| hole != 0 && !outer[hole as usize]).collect();
        // A lit row across the surface, a selected item of a list, is furniture of it: the list is
        // one room, not two parted by the thin margin the surface goes round the row by. A table
        // is furniture of its room whole, whatever its columns cut the surface into.
        for band in self.lit_bands(&own, crop, rect).into_iter().chain(tables) {
            for y in band.top..band.bottom {
                let start = (y - crop.top) as usize * cw + (band.left - crop.left) as usize;
                for i in start..start + band.width() as usize {
                    furniture[i] |= !own[i];
                }
            }
        }
        let space: Vec<bool> = own.iter().zip(&furniture).map(|(own, furniture)| *own || *furniture).collect();
        let distance = distances(&space, cw, false);
        let mut labels = basins(&distance, cw, self.scale * LEAST_DOOR);
        bands(&mut labels, &furniture, cw, self.px(FRAME), (200.0 * self.scale * self.scale) as u32, self.px(20.0));
        let count = labels.iter().copied().max().unwrap_or(0);
        let boxes = measure(&labels, cw, count).into_iter().map(|rect| rect.offset(crop.left, crop.top)).collect();
        Rooms { crop, labels, furniture, boxes, furnished: HashMap::new() }
    }

    /// A room's box and its blocks of furniture, once: small pieces, letters and icons, joined
    /// into words and lines into paragraphs; a large piece, a lit row or a picture, a block of its
    /// own.
    fn furnished(&mut self, rooms: usize, room_id: u32) -> Rc<Furnished> {
        if let Some(furnished) = self.rooms[rooms].furnished.get(&room_id) {
            return furnished.clone();
        }
        let furnished = Rc::new(self.furnish(&self.rooms[rooms], room_id));
        self.rooms[rooms].furnished.insert(room_id, furnished.clone());
        furnished
    }

    fn furnish(&self, rooms: &Rooms, room_id: u32) -> Furnished {
        let room = self.body(rooms, room_id);
        let (w, h) = (room.width() as usize, room.height() as usize);
        let width = rooms.crop.width() as usize;
        let mut inner = vec![false; w * h];
        for y in 0..h {
            let start = (room.top - rooms.crop.top) as usize * width + y * width + (room.left - rooms.crop.left) as usize;
            for x in 0..w {
                inner[y * w + x] = rooms.furniture[start + x] && rooms.labels[start + x] == room_id;
            }
        }
        let pieces = parts(&inner, w, true);
        let letter = self.px(LETTER);
        let small: Vec<bool> = pieces.boxes.iter().enumerate().map(|(n, b)| n > 0 && b.width() <= letter && b.height() <= letter).collect();
        let letters: Vec<bool> = pieces.labels.iter().map(|&n| small[n as usize]).collect();
        let join = self.px(JOIN) as usize;
        let grown_letters = grown(&letters, w, join, join);
        let joined: Vec<bool> = (0..w * h).map(|i| grown_letters[i] && (letters[i] || !inner[i])).collect();
        let Parts { labels: mut blocks, count: words, .. } = parts(&joined, w, true);
        for i in 0..w * h {
            if inner[i] && !letters[i] {
                blocks[i] = pieces.labels[i] + words;
            }
        }
        // How much furniture there is in any box of the room.
        let mut sums = vec![0u32; (w + 1) * (h + 1)];
        for y in 0..h {
            for x in 0..w {
                let here = inner[y * w + x] as u32;
                sums[(y + 1) * (w + 1) + x + 1] = here + sums[y * (w + 1) + x + 1] + sums[(y + 1) * (w + 1) + x] - sums[y * (w + 1) + x];
            }
        }
        let mut boxes = HashMap::new();
        for (n, b) in measure(&blocks, w, words + pieces.count).iter().enumerate().skip(1) {
            if b.is_empty() {
                continue;
            }
            let sum = |x: i32, y: i32| sums[y as usize * (w + 1) + x as usize] as i64;
            let filled = sum(b.right, b.bottom) - sum(b.left, b.bottom) - sum(b.right, b.top) + sum(b.left, b.top);
            if filled != 0 {
                boxes.insert(n as u32, (b.offset(room.left, room.top), (filled as f32) < SPARSE * size(*b)));
            }
        }
        Furnished { room, blocks, boxes }
    }

    /// The box of a room without its thin outgrowths: the spaces between the buttons of a toolbar
    /// on the same surface, a margin along a panel.
    fn body(&self, rooms: &Rooms, room_id: u32) -> Rect {
        let rect = rooms.boxes[room_id as usize];
        let (w, h) = (rect.width() as usize, rect.height() as usize);
        let width = rooms.crop.width() as usize;
        let mut mask = vec![false; w * h];
        for y in 0..h {
            let start = (rect.top - rooms.crop.top) as usize * width + y * width + (rect.left - rooms.crop.left) as usize;
            for x in 0..w {
                mask[y * w + x] = rooms.labels[start + x] == room_id;
            }
        }
        let depth = distances(&mask, w, true);
        let mut deepest = 0;
        for (y, row) in depth.chunks_exact(w).enumerate() {
            for (x, &d) in row.iter().enumerate() {
                deepest = deepest.max(d.min((x.min(w - 1 - x).min(y).min(h - 1 - y) + 1) as u32 * UNIT));
            }
        }
        let radius = (0.2 * deepest as f32 / UNIT as f32).max(4.0 * self.scale).min(40.0 * self.scale).floor();
        // What a circle of that radius reaches rolling inside the room, past the sides of its box
        // too.
        let mut opened = NOWHERE;
        for (y, row) in depth.chunks_exact(w).enumerate() {
            let inside = |d: &u32| *d > radius as u32 * UNIT;
            if let (Some(first), Some(last)) = (row.iter().position(inside), row.iter().rposition(inside)) {
                let (left, right) = (opened.left.min(first as i32), opened.right.max(last as i32 + 1));
                opened = Rect::new(left, opened.top.min(y as i32), right, y as i32 + 1);
            }
        }
        if opened.is_empty() {
            return rect;
        }
        opened.inflate(radius as i32).intersect(Rect::new(0, 0, w as i32, h as i32)).offset(rect.left, rect.top)
    }

    /// Whether just outside a box is busy, a wallpaper or a picture: a message of one line on it
    /// is an area, lower as it is than a panel.
    fn busy_around(&self, rect: Rect) -> bool {
        let out = self.px(3.0);
        let (mut all, mut apart) = (0u32, 0u32);
        let mut count = |points: &mut dyn Iterator<Item = (i32, i32)>| {
            let mut last: Option<[u8; 3]> = None;
            for (x, y) in points {
                let pixel = self.pixels[self.index(x, y)];
                if let Some(last) = last {
                    all += 1;
                    apart += !alike(last, pixel, SAME) as u32;
                }
                last = Some(pixel);
            }
        };
        if rect.top >= out {
            count(&mut (rect.left..rect.right).map(|x| (x, rect.top - out)));
        }
        if rect.bottom + out <= self.h {
            count(&mut (rect.left..rect.right).map(|x| (x, rect.bottom + out - 1)));
        }
        if rect.left >= out {
            count(&mut (rect.top..rect.bottom).map(|y| (rect.left - out, y)));
        }
        if rect.right + out <= self.w {
            count(&mut (rect.top..rect.bottom).map(|y| (rect.right + out - 1, y)));
        }
        all > 0 && apart as f32 >= BUSY * all as f32
    }

    /// The box with the line along each of its sides taken in, if the line is its own: thin, of
    /// another colour than both the box and what is past it, and no longer than the side, as the
    /// border of a card.
    fn bordered(&self, area: Rect) -> Rect {
        let thin = self.px(LINE).max(1);
        let reach = self.px(SLACK);
        // The pixel a distance out from a side, 0 for the last one in, at a place along it.
        let pixel = |side: usize, out: i32, along: i32| {
            let (x, y) = match side {
                0 => (area.left - out, along),
                1 => (area.right - 1 + out, along),
                2 => (along, area.top - out),
                _ => (along, area.bottom - 1 + out),
            };
            self.bounds().contains(Point::new(x, y)).then(|| self.pixels[self.index(x, y)])
        };
        let mut sides = [area.left, area.right, area.top, area.bottom];
        for (side, edge) in sides.iter_mut().enumerate() {
            let (start, end) = if side < 2 { (area.top, area.bottom) } else { (area.left, area.right) };
            for thickness in 1..=thin {
                let own = (start..end).filter(|&along| {
                    let (Some(inside), Some(line), Some(last)) = (pixel(side, 0, along), pixel(side, 1, along), pixel(side, thickness, along)) else {
                        return false;
                    };
                    !alike(inside, line, 2 * SAME)
                        && alike(line, last, SAME)
                        && pixel(side, thickness + 1, along).is_none_or(|past| !alike(past, last, 2 * SAME))
                });
                if (own.count() as f32) < MOST * (end - start) as f32 {
                    continue;
                }
                // A line that goes on past the ends of the side is not the box's own.
                let goes_on = |along: i32, next: i32| {
                    matches!((pixel(side, 1, along), pixel(side, 1, next)), (Some(line), Some(past)) if alike(line, past, SAME))
                };
                if !goes_on(start + (end - start) / 2, start - reach) && !goes_on(start + (end - start) / 2, end - 1 + reach) {
                    *edge += if side % 2 == 0 { -thickness } else { thickness };
                }
                break;
            }
        }
        Rect::new(sides[0], sides[2], sides[1], sides[3])
    }

    /// The tables of the window, by their rhythm: rows of text with three cells or more each, row
    /// after row at one step, the cells in columns that begin or end at the same places; the rows
    /// of its heading over them, whatever their step. The colour of a cell, as a column tinted by
    /// its values, doesn't count. A table across most of its rows' background is as wide as it.
    fn find_tables(&mut self) -> Vec<Rect> {
        let width = self.w as usize;
        let busy: Vec<bool> = self.marks.iter().map(|&mark| mark != 0).collect();
        let pieces = parts(&busy, width, true);
        // A letter, or a word whose letters the smoothing of the font has joined: no taller than
        // a line of text, however wide.
        let letter = self.px(LETTER);
        let small: Vec<bool> = pieces.boxes.iter().enumerate().map(|(n, b)| n > 0 && b.height() <= letter).collect();
        let letters: Vec<bool> = pieces.labels.iter().map(|&n| small[n as usize]).collect();
        let words = grown(&letters, width, self.px(JOIN) as usize, 0);
        let (low, high) = (self.px(6.0), self.px(30.0));
        let mut found = Vec::new();
        for rect in parts(&words, width, true).boxes.into_iter().skip(1) {
            // A line of text, not the grain of a picture pressed small.
            if rect.height() <= high && rect.height() >= low && rect.width() >= low {
                found.push(Line { middle: (rect.top + rect.bottom) as f32 / 2.0, rect });
            }
        }
        found.sort_by(|a, b| a.middle.total_cmp(&b.middle).then(a.rect.left.cmp(&b.rect.left)));
        let close = self.px(LINE + 1.0) as f32;
        let mut rows: Vec<Vec<Line>> = Vec::new();
        for line in found {
            match rows.last_mut() {
                Some(row) if line.middle - row[row.len() - 1].middle <= close => row.push(line),
                _ => rows.push(vec![line]),
            }
        }
        // A row ends where another surface comes between its cells: the lines of a list of chats
        // and of the messages beside it are not one row.
        let mut segments: Vec<Vec<Line>> = Vec::new();
        for mut row in rows {
            // Text on a smooth background: not the drawings of a wallpaper, in rows as its pattern
            // is.
            if row.len() >= 3 {
                row.retain(|line| {
                    let middle = ((line.rect.left + line.rect.right) / 2, line.middle as i32);
                    self.surface_at(middle.0, middle.1).is_some_and(|(colour, seed)| !self.gradient(colour, seed))
                });
            }
            if row.len() < 3 {
                continue;
            }
            row.sort_by_key(|line| line.rect.left);
            let mut part = vec![row[0]];
            for pair in row.windows(2) {
                if self.foreign_between(pair[0], pair[1]) {
                    segments.push(std::mem::take(&mut part));
                }
                part.push(pair[1]);
            }
            segments.push(part);
        }
        segments.retain(|part| part.len() >= 3);
        segments.sort_by(|a, b| at(a).total_cmp(&at(b)));
        let slack = self.px(SLACK);
        let furthest = self.px(80.0) as f32;
        // Rows after rows at one step, each table on its own track: two tables side by side.
        let mut tracks: Vec<Vec<Vec<Line>>> = Vec::new();
        'parts: for part in &segments {
            for run in &mut tracks {
                let step = at(part) - at(&run[run.len() - 1]);
                let first = if run.len() > 1 { at(&run[1]) - at(&run[0]) } else { step };
                if step > 0.0 && step <= furthest && in_columns(part, &run[run.len() - 1], slack) && (step - first).abs() <= 0.25 * first {
                    run.push(part.clone());
                    continue 'parts;
                }
            }
            tracks.push(vec![part.clone()]);
        }
        let mut tables = Vec::new();
        for mut run in tracks {
            if run.len() < 4 {
                continue;
            }
            let step = (at(&run[run.len() - 1]) - at(&run[0])) / (run.len() - 1) as f32;
            // The heading over the rows: rows in the same columns just above, a step apart or a
            // little more, as a heading's line of percentages over the names of the columns; its
            // names a little off the cells' text, past the lines between the columns of the
            // heading.
            for _ in 0..2 {
                let over = segments.iter().filter(|part| {
                    let above = at(&run[0]) - at(part);
                    above > 0.0 && above <= 1.5 * step && in_columns(part, &run[0], 2 * slack)
                });
                let Some(part) = over.max_by(|a, b| at(a).total_cmp(&at(b))) else { break };
                run.insert(0, part.clone());
            }
            // Only the columns of most of the rows: not the items of a panel beside the table that
            // happen to stand at the same height in some of them.
            let in_column = |line: &Line, row: &Vec<Line>| {
                row.iter().any(|other| (line.rect.left - other.rect.left).abs() <= slack || (line.rect.right - other.rect.right).abs() <= slack)
            };
            let kept: Vec<Line> = run
                .iter()
                .flatten()
                .copied()
                .filter(|line| 2 * run.iter().filter(|row| in_column(line, row)).count() >= run.len())
                .collect();
            if kept.is_empty() {
                continue;
            }
            let third = (step / 3.0) as i32;
            let top = (run[0].iter().map(|line| line.rect.top).min().unwrap_or(0) - third).max(0);
            let mut bottom = (run[run.len() - 1].iter().map(|line| line.rect.bottom).max().unwrap_or(0) + third).min(self.h);
            // Rows that run on past the window's edge, the last of them cut by it.
            if (self.h - bottom) as f32 <= step {
                bottom = self.h;
            }
            let mut left = (kept.iter().map(|line| line.rect.left).min().unwrap_or(0) - slack).max(0);
            let mut right = (kept.iter().map(|line| line.rect.right).max().unwrap_or(0) + slack).min(self.w);
            // How far the rows' background goes, past the text of their first and last cells.
            let (mut lefts, mut rights) = (Vec::new(), Vec::new());
            for part in [&run[run.len() / 4], &run[run.len() / 2], &run[3 * run.len() / 4]] {
                let first = part.iter().min_by_key(|line| line.rect.left).copied().unwrap_or(part[0]);
                let last = part.iter().rev().max_by_key(|line| line.rect.right).copied().unwrap_or(part[0]);
                let colours: Vec<Color> = [first, last]
                    .iter()
                    .filter_map(|line| self.colour_at((line.rect.left + line.rect.right) / 2, line.middle as i32))
                    .map(|(colour, _)| colour)
                    .collect();
                if !colours.is_empty() {
                    lefts.push(self.reach(first.middle as i32, first.rect.left, -1, &colours));
                    rights.push(self.reach(last.middle as i32, last.rect.right, 1, &colours));
                }
            }
            if !lefts.is_empty() {
                let (reach_left, reach_right) = (median(&mut lefts), median(&mut rights));
                if (right - left) as f32 >= MOST_WIDE * (reach_right - reach_left) as f32 {
                    (left, right) = (left.min(reach_left), right.max(reach_right));
                }
            }
            tables.push(Rect::new(left, top, right, bottom));
        }
        tables
    }

    /// Whether another surface comes between two lines of a row, a pane between two panels: more
    /// than a line's width of a colour that is the background of neither.
    fn foreign_between(&self, a: Line, b: Line) -> bool {
        let (x0, x1) = (a.rect.right, b.rect.left);
        let slack = self.px(SLACK);
        if x1 - x0 <= slack {
            return false;
        }
        let colour = |line: Line| self.colour_at((line.rect.left + line.rect.right) / 2, line.middle as i32).map(|(colour, _)| colour);
        let (Some(ca), Some(cb)) = (colour(a), colour(b)) else { return false };
        let y = ((a.middle + b.middle) / 2.0).round() as i32;
        let (mut run, mut longest) = (0, 0);
        for x in x0..x1 {
            let pixel = self.pixels[self.index(x, y)];
            run = if !of_colour(pixel, ca) && !of_colour(pixel, cb) { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        longest > slack
    }

    /// Where the background of a row from a point on ends, past thin lines across it: the
    /// backgrounds of its cells, a tinted one and the row's own.
    fn reach(&self, y: i32, mut x: i32, direction: i32, colours: &[Color]) -> i32 {
        let slack = self.px(SLACK);
        let mut run = 0;
        while x + direction >= 0 && x + direction < self.w {
            x += direction;
            if colours.iter().all(|&colour| !of_colour(self.pixels[self.index(x, y)], colour)) {
                run += 1;
                if run > slack {
                    return x - direction * (run - 1);
                }
            } else {
                run = 0;
            }
        }
        x
    }
}

/// The height of a row of a table.
fn at(row: &[Line]) -> f32 {
    row.iter().map(|line| line.middle).fold(f32::MAX, f32::min)
}

/// Whether two rows of a table have two cells at least that begin or end at the same places.
fn in_columns(a: &[Line], b: &[Line], slack: i32) -> bool {
    let starts = a.iter().filter(|p| b.iter().any(|q| (p.rect.left - q.rect.left).abs() <= slack)).count();
    let ends = a.iter().filter(|p| b.iter().any(|q| (p.rect.right - q.rect.right).abs() <= slack)).count();
    starts.max(ends) >= 2
}

/// The value in the middle.
fn median(values: &mut [i32]) -> i32 {
    values.sort_unstable();
    let n = values.len();
    if n % 2 == 1 { values[n / 2] } else { (values[n / 2 - 1] + values[n / 2]) / 2 }
}

fn size(rect: Rect) -> f32 {
    rect.width() as f32 * rect.height() as f32
}

/// Whether two colours are of one surface, within `by` in every channel.
fn alike(a: [u8; 3], b: [u8; 3], by: u8) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= by)
}

/// The tone of a colour.
fn tone_of(colour: Color) -> Tone {
    colour.map(|c| (c as u32 / 8) as u8)
}

/// Whether a pixel is of a surface of the colour.
fn of_colour(pixel: [u8; 3], colour: Color) -> bool {
    pixel.iter().zip(colour).all(|(p, c)| (*p as f32 - c).abs() <= TOLERANCE)
}

/// What a label stands for now that others have joined it.
fn root(parent: &mut [u32], mut n: u32) -> u32 {
    while parent[n as usize] != n {
        parent[n as usize] = parent[parent[n as usize] as usize];
        n = parent[n as usize];
    }
    n
}

/// The connected parts of a mask, numbered from 1 in the order they begin.
struct Parts {
    /// The part of every pixel, 0 for none.
    labels: Vec<u32>,
    count: u32,
    /// How many pixels each part has and its box, by its number; nothing for number 0.
    sizes: Vec<u32>,
    boxes: Vec<Rect>,
}

/// A box of nothing, for a number no pixel has.
const NOWHERE: Rect = Rect::new(1 << 30, 1 << 30, -1 << 30, -1 << 30);

/// The connected parts of a mask of `width` columns, of pixels touching by a side and, if
/// `diagonal`, by a corner.
fn parts(mask: &[bool], width: usize, diagonal: bool) -> Parts {
    let height = mask.len() / width.max(1);
    let reach = diagonal as usize;
    // The runs of the mask along its rows: row, start, end and label.
    let mut found: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut parent = vec![0u32];
    let (mut above, mut above_end) = (0, 0);
    for y in 0..height {
        let row = &mask[y * width..y * width + width];
        let first = found.len();
        let mut x = 0;
        while x < width {
            let Some(start) = row[x..].iter().position(|on| *on) else { break };
            let start = x + start;
            let end = row[start..].iter().position(|on| !*on).map_or(width, |n| start + n);
            let mut here = 0;
            // The runs of the row above that touch this one.
            while above < above_end && found[above].2 as usize + reach <= start {
                above += 1;
            }
            for &(_, from, _, label) in &found[above..above_end] {
                if from as usize >= end + reach {
                    break;
                }
                let other = root(&mut parent, label);
                if here == 0 {
                    here = other;
                } else if other != here {
                    let (low, high) = (here.min(other), here.max(other));
                    parent[high as usize] = low;
                    here = low;
                }
            }
            if here == 0 {
                here = parent.len() as u32;
                parent.push(here);
            }
            found.push((y as u32, start as u32, end as u32, here));
            x = end;
        }
        (above, above_end) = (first, found.len());
    }
    let mut numbers = vec![0u32; parent.len()];
    let mut count = 0;
    for n in 1..parent.len() {
        let top = root(&mut parent, n as u32) as usize;
        if numbers[top] == 0 {
            count += 1;
            numbers[top] = count;
        }
        numbers[n] = numbers[top];
    }
    let mut labels = vec![0u32; mask.len()];
    let mut sizes = vec![0u32; count as usize + 1];
    let mut boxes = vec![NOWHERE; count as usize + 1];
    for (y, from, to, label) in found {
        let number = numbers[label as usize];
        labels[y as usize * width + from as usize..y as usize * width + to as usize].fill(number);
        sizes[number as usize] += to - from;
        let rect = &mut boxes[number as usize];
        (rect.left, rect.top) = (rect.left.min(from as i32), rect.top.min(y as i32));
        (rect.right, rect.bottom) = (rect.right.max(to as i32), rect.bottom.max(y as i32 + 1));
    }
    Parts { labels, count, sizes, boxes }
}

/// The box of every label, nothing for no label at all.
fn measure(labels: &[u32], width: usize, count: u32) -> Vec<Rect> {
    let mut boxes = vec![NOWHERE; count as usize + 1];
    for (y, row) in labels.chunks_exact(width).enumerate() {
        let y = y as i32;
        for (x, &label) in row.iter().enumerate() {
            if label != 0 {
                let (x, rect) = (x as i32, &mut boxes[label as usize]);
                (rect.left, rect.top) = (rect.left.min(x), rect.top.min(y));
                (rect.right, rect.bottom) = (rect.right.max(x + 1), rect.bottom.max(y + 1));
            }
        }
    }
    boxes
}

/// What is left of a mask but runs at least `least` long, along its rows or else down its columns.
fn runs(mask: &[bool], width: usize, least: usize, along_rows: bool) -> Vec<bool> {
    let height = mask.len() / width.max(1);
    let mut kept = vec![false; mask.len()];
    if along_rows {
        for (row, out) in mask.chunks_exact(width).zip(kept.chunks_exact_mut(width)) {
            let mut run = 0;
            for x in 0..=width {
                if x < width && row[x] {
                    run += 1;
                    continue;
                }
                if run >= least {
                    out[x - run..x].fill(true);
                }
                run = 0;
            }
        }
        return kept;
    }
    let mut lengths = vec![0usize; width];
    for y in 0..=height {
        for (x, length) in lengths.iter_mut().enumerate() {
            if y < height && mask[y * width + x] {
                *length += 1;
                continue;
            }
            if *length >= least {
                for k in y - *length..y {
                    kept[k * width + x] = true;
                }
            }
            *length = 0;
        }
    }
    kept
}

/// A mask grown by `across` pixels along its rows and `down` pixels along its columns.
fn grown(mask: &[bool], width: usize, across: usize, down: usize) -> Vec<bool> {
    let height = mask.len() / width.max(1);
    let mut wide = vec![false; mask.len()];
    for y in 0..height {
        let row = &mask[y * width..y * width + width];
        let out = &mut wide[y * width..y * width + width];
        let mut last = None;
        for x in 0..width {
            if row[x] {
                last = Some(x);
            }
            out[x] = last.is_some_and(|last| x - last <= across);
        }
        let mut next = None;
        for x in (0..width).rev() {
            if row[x] {
                next = Some(x);
            }
            out[x] |= next.is_some_and(|next| next - x <= across);
        }
    }
    if down == 0 {
        return wide;
    }
    let mut tall = vec![false; mask.len()];
    let mut last = vec![None; width];
    for y in 0..height {
        for x in 0..width {
            if wide[y * width + x] {
                last[x] = Some(y);
            }
            tall[y * width + x] = last[x].is_some_and(|last| y - last <= down);
        }
    }
    let mut next = vec![None; width];
    for y in (0..height).rev() {
        for x in 0..width {
            if wide[y * width + x] {
                next[x] = Some(y);
            }
            tall[y * width + x] |= next[x].is_some_and(|next: usize| next - y <= down);
        }
    }
    tall
}

/// How far every pixel of a mask is from the nearest pixel outside it, in `UNIT`s to the pixel, by
/// the steps from pixel to pixel in a close approximation of straight lines. What is past the
/// sides of the mask is outside it, unless `open`.
fn distances(mask: &[bool], width: usize, open: bool) -> Vec<u32> {
    const FAR: u32 = u32::MAX / 2;
    const STRAIGHT: u32 = UNIT;
    const DIAGONAL: u32 = UNIT * 14 / 10;
    const KNIGHT: u32 = UNIT * 21969 / 10000;
    let height = mask.len() / width.max(1);
    // Two rows and columns around it.
    let stride = width + 4;
    let mut d = vec![if open { FAR } else { 0 }; stride * (height + 4)];
    for (y, row) in mask.chunks_exact(width).enumerate() {
        for (cell, &on) in d[(y + 2) * stride + 2..].iter_mut().zip(row) {
            *cell = if on { FAR } else { 0 };
        }
    }
    // Each pixel from those before it, and then from those after it, row by row.
    for y in 2..height + 2 {
        let (done, rest) = d.split_at_mut(y * stride);
        let (far, near) = done[(y - 2) * stride..].split_at(stride);
        let row = &mut rest[..stride];
        let mut last = row[1];
        for x in 2..width + 2 {
            if row[x] != 0 {
                row[x] = row[x]
                    .min(last + STRAIGHT)
                    .min(near[x] + STRAIGHT)
                    .min(near[x - 1].min(near[x + 1]) + DIAGONAL)
                    .min(near[x - 2].min(near[x + 2]).min(far[x - 1]).min(far[x + 1]) + KNIGHT);
            }
            last = row[x];
        }
    }
    for y in (2..height + 2).rev() {
        let (rest, done) = d.split_at_mut((y + 1) * stride);
        let (near, far) = done.split_at(stride);
        let row = &mut rest[y * stride..];
        let mut last = row[width + 2];
        for x in (2..width + 2).rev() {
            if row[x] != 0 {
                row[x] = row[x]
                    .min(last + STRAIGHT)
                    .min(near[x] + STRAIGHT)
                    .min(near[x - 1].min(near[x + 1]) + DIAGONAL)
                    .min(near[x - 2].min(near[x + 2]).min(far[x - 1]).min(far[x + 1]) + KNIGHT);
            }
            last = row[x];
        }
    }
    let mut out = Vec::with_capacity(mask.len());
    for y in 0..height {
        let start = (y + 2) * stride + 2;
        out.extend_from_slice(&d[start..start + width]);
    }
    out
}

/// The rooms of a space, by its distance from the walls, a label for every pixel: the basins
/// around its widest places, poured from the widest down, two of them one room unless the way
/// between them narrows from the narrower one's widest by a door's depth, a quarter of it. A
/// narrow panel beside a wide list is a room of its own, and the middle of a long room, rippling
/// by a pixel or dented by a line under a tab, is one. A space too thin for a door is no room.
fn basins(distance: &[u32], width: usize, least_door: f32) -> Vec<u32> {
    // The level of every pixel of the space, widest first by quarters of a pixel, 0 for none, with
    // a row and a column of nothing around them.
    const FINENESS: u32 = UNIT / 4;
    const WAITING: u32 = u32::MAX;
    let widest = distance.iter().copied().max().unwrap_or(0);
    let top = widest / FINENESS + 1;
    let height = distance.len() / width.max(1);
    let stride = width + 2;
    let mut levels = vec![0u32; stride * (height + 2)];
    let mut starts = vec![0u32; top as usize + 2];
    for (y, row) in distance.chunks_exact(width).enumerate() {
        for (x, &d) in row.iter().enumerate() {
            if d > 0 {
                let level = top - d / FINENESS;
                levels[(y + 1) * stride + x + 1] = level;
                starts[level as usize + 1] += 1;
            }
        }
    }
    for n in 1..starts.len() {
        starts[n] += starts[n - 1];
    }
    let mut order = vec![0u32; starts[top as usize + 1] as usize];
    let mut next = starts.clone();
    for (i, &level) in levels.iter().enumerate() {
        if level != 0 {
            order[next[level as usize] as usize] = i as u32;
            next[level as usize] += 1;
        }
    }
    let beside = |i: usize| [i - 1, i + 1, i - stride, i + stride];
    let mut labels = vec![0u32; levels.len()];
    let (mut parent, mut peak) = (vec![0u32], vec![0f32]);
    let mut queue = VecDeque::new();
    let poured = |label: u32| label != 0 && label != WAITING;
    for here in 1..=top {
        let pixels = &order[starts[here as usize] as usize..starts[here as usize + 1] as usize];
        // A level is poured from what is poured already evenly along it, so that two basins share
        // a long room's middle, all of one level, rather than the first taking it all.
        for &i in pixels {
            let i = i as usize;
            if beside(i).iter().any(|&n| poured(labels[n])) {
                labels[i] = WAITING;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            // The pixel goes with the neighbour nearest the widest place of its own.
            let nearest = beside(i).into_iter().filter(|&n| poured(labels[n])).min_by_key(|&n| levels[n]);
            let Some(nearest) = nearest else { continue };
            let first = labels[nearest];
            let own = root(&mut parent, first);
            let width_here = (top - here) as f32 / 4.0;
            for n in beside(i) {
                if poured(labels[n]) {
                    if labels[n] == first || labels[n] == own {
                        continue;
                    }
                    let other = root(&mut parent, labels[n]);
                    let narrower = peak[own as usize].min(peak[other as usize]);
                    if other != own && narrower - width_here < least_door.max(DOOR * narrower) {
                        peak[own as usize] = peak[own as usize].max(peak[other as usize]);
                        parent[other as usize] = own;
                    }
                } else if labels[n] == 0 && levels[n] == here {
                    labels[n] = WAITING;
                    queue.push_back(n);
                }
            }
            labels[i] = own;
        }
        // What is left of the level is widest places of their own.
        for &i in pixels {
            let i = i as usize;
            if labels[i] != 0 {
                continue;
            }
            let new = parent.len() as u32;
            parent.push(new);
            peak.push((top - here) as f32 / 4.0);
            labels[i] = new;
            queue.push_back(i);
            while let Some(i) = queue.pop_front() {
                for n in beside(i) {
                    if labels[n] == 0 && levels[n] == here {
                        labels[n] = new;
                        queue.push_back(n);
                    }
                }
            }
        }
    }
    let mut numbers = vec![0u32; parent.len()];
    let mut count = 0;
    for n in 1..parent.len() {
        let top = root(&mut parent, n as u32) as usize;
        if numbers[top] == 0 && peak[top] >= least_door {
            count += 1;
            numbers[top] = count;
        }
    }
    let mut rooms = Vec::with_capacity(distance.len());
    for y in 0..height {
        for &label in &labels[(y + 1) * stride + 1..(y + 1) * stride + 1 + width] {
            rooms.push(if label != 0 { numbers[root(&mut parent, label) as usize] } else { 0 });
        }
    }
    rooms
}

/// A band across rooms side by side is a room of its own, cut off along the side of what stands on
/// it: what lies in two rooms at once, as an address field over a navigation pane and a list, is a
/// part of neither, and they begin under it; and the wide top or bottom of a room shaped as a T,
/// as a menu bar over panels, is cut off from its stem, those of rooms side by side at the same
/// heights into one band: toolbars over the margins on either side of a page. `frame` is how far
/// from their ends rooms are cut, `least` the pixels of what cuts them, `wider` how much wider
/// than its stem a T's top is, a quarter of the stem besides.
fn bands(labels: &mut [u32], furniture: &[bool], width: usize, frame: i32, least: u32, wider: i32) {
    let height = labels.len() / width.max(1);
    let mut count = labels.iter().copied().max().unwrap_or(0);
    let pieces = parts(furniture, width, true);
    let mut rooms = measure(labels, width, count);
    for piece in 1..=pieces.count {
        let (size, rect) = (pieces.sizes[piece as usize], pieces.boxes[piece as usize]);
        if size < least {
            continue;
        }
        let mut shares: HashMap<u32, u32> = HashMap::new();
        for y in rect.top as usize..rect.bottom as usize {
            for x in rect.left as usize..rect.right as usize {
                let i = y * width + x;
                if pieces.labels[i] == piece && labels[i] != 0 {
                    *shares.entry(labels[i]).or_default() += 1;
                }
            }
        }
        let under: Vec<u32> = shares.iter().filter(|(_, share)| **share as f32 >= 0.15 * size as f32).map(|(room, _)| *room).collect();
        if under.len() < 2 {
            continue;
        }
        let top = under.iter().map(|&room| rooms[room as usize].top).min().unwrap_or(0);
        let bottom = under.iter().map(|&room| rooms[room as usize].bottom).max().unwrap_or(0);
        let (from, to) = if rect.top - top <= frame {
            (0, rect.bottom as usize)
        } else if bottom - rect.bottom <= frame {
            (rect.top as usize, height)
        } else {
            continue;
        };
        count += 1;
        for label in &mut labels[from * width..to * width] {
            if under.contains(label) {
                *label = count;
            }
        }
        rooms = measure(labels, width, count);
    }
    // The wide top or bottom of a room shaped as a T.
    let mut cut: Vec<(i32, i32, u32)> = Vec::new();
    let rooms_cut = count;
    for room in 1..=rooms_cut {
        let rect = rooms[room as usize];
        if rect.is_empty() {
            continue;
        }
        let row = |labels: &[u32], y: i32| {
            let start = y as usize * width + rect.left as usize;
            let cells = &labels[start..start + rect.width() as usize];
            let first = cells.iter().position(|&l| l == room);
            let last = cells.iter().rposition(|&l| l == room);
            first.zip(last).map_or(0, |(first, last)| (last + 1 - first) as i32)
        };
        let widths: Vec<i32> = (rect.top..rect.bottom).map(|y| row(labels, y)).collect();
        let mut filled: Vec<i32> = widths.iter().copied().filter(|&w| w > 0).collect();
        filled.sort_unstable();
        let n = filled.len();
        let stem = if n % 2 == 1 { filled[n / 2] as f32 } else { (filled[n / 2 - 1] + filled[n / 2]) as f32 / 2.0 };
        let wide = |w: i32| w as f32 >= 1.25 * stem + wider as f32;
        let from_top = widths.iter().take_while(|&&w| wide(w)).count() as i32;
        let from_bottom = widths.iter().rev().take_while(|&&w| wide(w)).count() as i32;
        let thin = |band: i32| band > 0 && band <= frame && 2 * band < rect.height();
        let cuts = [(thin(from_top), rect.top, rect.top + from_top), (thin(from_bottom), rect.bottom - from_bottom, rect.bottom)];
        for (low, high) in cuts.into_iter().filter(|cut| cut.0).map(|cut| (cut.1, cut.2)) {
            // What is beside the band, if it is cut off already.
            let mut beside = None;
            for y in low..high {
                let start = y as usize * width;
                for x in 0..width {
                    if labels[start + x] == room {
                        let around = [(x > 0).then(|| labels[start + x - 1]), (x + 1 < width).then(|| labels[start + x + 1])];
                        for other in around.into_iter().flatten() {
                            if cut.iter().any(|&(top, bottom, band)| band == other && (top - low).abs() <= 2 && (bottom - high).abs() <= 2) {
                                beside = Some(other);
                            }
                        }
                    }
                }
            }
            let band = beside.unwrap_or_else(|| {
                count += 1;
                cut.push((low, high, count));
                count
            });
            for label in &mut labels[low as usize * width..high as usize * width] {
                if *label == room {
                    *label = band;
                }
            }
        }
    }
}

/// The picture with every channel of every pixel the median of those of the pixels `radius` around
/// it, the pixels at its sides repeated past them: small drawings, letters and grain go.
fn smoothed(pixels: &[[u8; 3]], width: usize, radius: usize) -> Vec<[u8; 3]> {
    let height = pixels.len() / width.max(1);
    let mut smooth = vec![[0u8; 3]; pixels.len()];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let rows = height.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (n, strip) in smooth.chunks_mut(rows * width).enumerate() {
            scope.spawn(move || {
                for (k, row) in strip.chunks_mut(width).enumerate() {
                    smooth_row(pixels, width, n * rows + k, radius, row);
                }
            });
        }
    });
    smooth
}

/// A row of the smoothed picture, keeping how many of the pixels around each pixel have each value
/// from one pixel to the next.
fn smooth_row(pixels: &[[u8; 3]], width: usize, y: usize, radius: usize, row: &mut [[u8; 3]]) {
    let height = pixels.len() / width;
    let r = radius as isize;
    let rows: Vec<usize> = (-r..=r).map(|k| (y as isize + k).clamp(0, height as isize - 1) as usize).collect();
    let column = |x: isize| x.clamp(0, width as isize - 1) as usize;
    let half = (rows.len() * rows.len() / 2) as u32;
    let mut counts = [[0u32; 256]; 3];
    for &ry in &rows {
        for k in -r..=r {
            let pixel = pixels[ry * width + column(k)];
            for c in 0..3 {
                counts[c][pixel[c] as usize] += 1;
            }
        }
    }
    let (mut median, mut below) = ([0usize; 3], [0u32; 3]);
    for x in 0..width {
        if x > 0 {
            for &ry in &rows {
                let gone = pixels[ry * width + column(x as isize - 1 - r)];
                let come = pixels[ry * width + column(x as isize + r)];
                for c in 0..3 {
                    counts[c][gone[c] as usize] -= 1;
                    below[c] -= ((gone[c] as usize) < median[c]) as u32;
                    counts[c][come[c] as usize] += 1;
                    below[c] += ((come[c] as usize) < median[c]) as u32;
                }
            }
        }
        for c in 0..3 {
            while below[c] > half {
                median[c] -= 1;
                below[c] -= counts[c][median[c]];
            }
            while below[c] + counts[c][median[c]] <= half {
                below[c] += counts[c][median[c]];
                median[c] += 1;
            }
            row[x][c] = median[c] as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A picture of the given size filled with one colour.
    fn canvas(width: u32, height: u32, color: [u8; 3]) -> Image {
        let pixels = (0..width * height).flat_map(|_| [color[0], color[1], color[2], 255]).collect();
        Image { width, height, pixels }
    }

    fn fill(image: &mut Image, rect: Rect, color: [u8; 3]) {
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let i = ((y as u32 * image.width + x as u32) * 4) as usize;
                image.pixels[i..i + 3].copy_from_slice(&color);
            }
        }
    }

    /// Lines of made-up text: words of dark strokes in rows, with the paper showing between them,
    /// as between the strokes of letters.
    fn text(image: &mut Image, rect: Rect, line: i32) {
        let mut y = rect.top;
        while y + line <= rect.bottom {
            let mut x = rect.left;
            let mut n = 0;
            while x < rect.right - 4 {
                let word = 12 + (n * 7) % 30;
                let end = (x + word).min(rect.right);
                for stroke in (x..end).step_by(4) {
                    fill(image, Rect::new(stroke, y + 3, (stroke + 2).min(end), y + line - 4), [40, 40, 40]);
                }
                x += word + 6;
                n += 1;
            }
            y += line + 4;
        }
    }

    /// A photograph: a texture with no smooth surface in it.
    fn photo(image: &mut Image, rect: Rect) {
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let v = ((x * 37 + y * 91) % 97) as u8 * 2;
                fill(image, Rect::new(x, y, x + 1, y + 1), [v, 255 - v, v / 2]);
            }
        }
    }

    /// A window made up for the tests: a title bar and a toolbar, a grey sidebar with a line on
    /// its right, and on white an article: a heading, two paragraphs, a photograph and a card with
    /// a border.
    fn window() -> Image {
        let mut image = canvas(1200, 800, [255, 255, 255]);
        fill(&mut image, Rect::new(0, 0, 1200, 32), [230, 230, 230]);
        fill(&mut image, Rect::new(0, 32, 1200, 80), [245, 245, 245]);
        fill(&mut image, Rect::new(0, 80, 1200, 81), [200, 200, 200]);
        fill(&mut image, Rect::new(0, 81, 260, 800), [240, 240, 240]);
        fill(&mut image, Rect::new(260, 81, 261, 800), [210, 210, 210]);
        text(&mut image, Rect::new(20, 100, 240, 400), 24);
        text(&mut image, Rect::new(300, 110, 700, 142), 32);
        text(&mut image, Rect::new(300, 170, 1150, 310), 24);
        text(&mut image, Rect::new(300, 340, 1150, 452), 24);
        photo(&mut image, Rect::new(300, 490, 700, 760));
        fill(&mut image, Rect::new(740, 490, 1150, 491), [190, 190, 190]);
        fill(&mut image, Rect::new(740, 759, 1150, 760), [190, 190, 190]);
        fill(&mut image, Rect::new(740, 490, 741, 760), [190, 190, 190]);
        fill(&mut image, Rect::new(1149, 490, 1150, 760), [190, 190, 190]);
        text(&mut image, Rect::new(760, 510, 1130, 740), 24);
        image
    }

    /// Prints the areas at points of a screenshot: `SCREENSHOT_AREAS_FILE=<file.png>
    /// SCREENSHOT_AREAS_POINTS="x,y x,y" cargo test -p ainitshot areas::tests::the_areas_at_points
    /// -- --exact --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_areas_at_points() {
        let file = std::env::var_os("SCREENSHOT_AREAS_FILE").expect("a screenshot");
        let image = Image::from_png(&std::fs::read(file).unwrap()).unwrap();
        let points = std::env::var("SCREENSHOT_AREAS_POINTS").expect("points");
        let mut areas = Areas::new(&image);
        for point in points.split_whitespace() {
            let (x, y) = point.split_once(',').unwrap();
            let p = Point::new(x.parse().unwrap(), y.parse().unwrap());
            let started = std::time::Instant::now();
            let area = areas.at(&image, image.bounds(), p, 1.0);
            println!("{p:?}: {area:?} in {:.1} ms", started.elapsed().as_secs_f32() * 1000.0);
        }
    }

    #[test]
    fn the_areas_of_a_window() {
        let image = window();
        let mut areas = Areas::new(&image);
        let whole = image.bounds();
        let at = |areas: &mut Areas, x, y| areas.at(&image, whole, Point::new(x, y), 1.0);
        // The photograph, the card with its border, a paragraph and the sidebar.
        assert_eq!(at(&mut areas, 500, 600), Some(Rect::new(300, 490, 700, 760)));
        assert_eq!(at(&mut areas, 900, 600), Some(Rect::new(740, 490, 1150, 760)));
        let paragraph = at(&mut areas, 700, 200).expect("no paragraph");
        assert!(paragraph.top >= 160 && paragraph.bottom <= 320 && paragraph.left >= 290, "{paragraph:?}");
        // The sidebar takes in the line on its side, its own border.
        let sidebar = at(&mut areas, 100, 600).expect("no sidebar");
        assert_eq!(sidebar, Rect::new(0, 81, 261, 800));
        // The toolbar is a band of the window's frame: the window, nothing of its own.
        assert_eq!(at(&mut areas, 600, 50), None);
    }

    #[test]
    fn the_messages_of_a_chat_on_a_patterned_wallpaper() {
        // A wallpaper of little drawings scattered on a green that grows lighter downwards, and on
        // it white messages with rounded corners: one of three lines, and one of a single line, a
        // strip.
        let mut image = canvas(900, 700, [0, 0, 0]);
        for y in 0..700 {
            let v = (y / 14) as u8;
            fill(&mut image, Rect::new(0, y, 900, y + 1), [120 + v, 165 + v, 115 + v]);
        }
        for k in 0..2500 {
            let (x, y) = ((k * 37) % 890, (k * 53) % 690);
            for d in 0..8 {
                fill(&mut image, Rect::new(x + d, y + d / 2, x + d + 1, y + d / 2 + 1), [90, 130, 90]);
            }
        }
        let bubble = |image: &mut Image, rect: Rect, lines: i32| {
            fill(image, rect, [255, 255, 255]);
            for (cx, cy) in [(rect.left, rect.top), (rect.right - 1, rect.top), (rect.left, rect.bottom - 1), (rect.right - 1, rect.bottom - 1)] {
                // The corner rounded off: the pixels beyond a quarter of a circle get the wallpaper.
                for dy in 0..8 {
                    for dx in 0..8 {
                        if (8 - dx) * (8 - dx) + (8 - dy) * (8 - dy) > 64 {
                            let x = if cx == rect.left { cx + dx } else { cx - dx };
                            let y = if cy == rect.top { cy + dy } else { cy - dy };
                            fill(image, Rect::new(x, y, x + 1, y + 1), [130, 175, 125]);
                        }
                    }
                }
            }
            text(image, Rect::new(rect.left + 12, rect.top + 8, rect.right - 12, rect.top + 8 + lines * 20), 16);
        };
        bubble(&mut image, Rect::new(100, 100, 520, 172), 3);
        bubble(&mut image, Rect::new(100, 200, 420, 234), 1);
        let mut areas = Areas::new(&image);
        let mut at = |x, y| areas.at(&image, image.bounds(), Point::new(x, y), 1.0);
        assert_eq!(at(300, 140), Some(Rect::new(100, 100, 520, 172)));
        assert_eq!(at(300, 217), Some(Rect::new(100, 200, 420, 234)));
        // The wallpaper around them is one surface, the top of it and the bottom alike.
        assert_eq!(at(700, 40), at(700, 660));
    }

    #[test]
    fn a_table_lights_up_whole_and_not_a_column_of_it() {
        // Ten rows in three columns, as files in a folder, with more space between the columns than
        // between the rows.
        let mut image = canvas(1000, 700, [255, 255, 255]);
        for row in 0..10 {
            let y = 100 + row * 30;
            for (left, right) in [(100, 300), (360, 480), (540, 640)] {
                text(&mut image, Rect::new(left, y, right, y + 16), 16);
            }
        }
        let mut areas = Areas::new(&image);
        let area = areas.at(&image, image.bounds(), Point::new(400, 200), 1.0).expect("no area");
        assert!(area.left <= 100 && area.right >= 640 && area.top <= 103 && area.bottom >= 380, "{area:?}");
    }

    #[test]
    fn a_column_tinted_by_its_values_does_not_cut_a_table() {
        // A table on white over a part of the window, with a heading, a line between the columns
        // of the heading, and two columns of cells tinted by their values, some of them a lot.
        let mut image = canvas(1100, 700, [255, 255, 255]);
        fill(&mut image, Rect::new(0, 0, 50, 700), [235, 235, 235]);
        for (left, right) in [(70, 200), (300, 380), (420, 500), (540, 620), (660, 760)] {
            text(&mut image, Rect::new(left, 60, right, 76), 16);
        }
        for x in [280, 400, 520, 640] {
            fill(&mut image, Rect::new(x, 50, x + 1, 90), [225, 225, 225]);
        }
        for row in 0..18 {
            let y = 90 + row * 28;
            let tint = |image: &mut Image, left, right, dark: bool| {
                fill(image, Rect::new(left, y, right, y + 28), if dark { [120, 170, 240] } else { [205, 230, 250] });
            };
            tint(&mut image, 281, 400, row % 5 == 1);
            tint(&mut image, 401, 520, row % 4 == 2);
            for (left, right) in [(70, 200), (300, 380), (420, 500), (540, 620), (660, 760)] {
                text(&mut image, Rect::new(left, y + 6, right, y + 22), 16);
            }
        }
        let mut areas = Areas::new(&image);
        let mut at = |x, y| areas.at(&image, image.bounds(), Point::new(x, y), 1.0);
        let table = at(130, 300).expect("no table");
        assert!(table.left <= 70 && table.right >= 760 && table.top <= 60 && table.bottom >= 590, "{table:?}");
        assert_eq!(at(340, 320), Some(table));
        assert_eq!(at(460, 124), Some(table));
    }

    #[test]
    fn a_selected_row_does_not_cut_a_list() {
        // A grey list down the side of a window, each item a line of text, one of them lit all
        // across the list and another with a line of light in it: the surface above the lit row
        // and the one below it are one room.
        let mut image = canvas(1000, 700, [255, 255, 255]);
        fill(&mut image, Rect::new(0, 0, 300, 700), [45, 45, 48]);
        fill(&mut image, Rect::new(300, 0, 301, 700), [90, 90, 90]);
        fill(&mut image, Rect::new(0, 300, 300, 332), [10, 80, 150]);
        for row in 0..20 {
            text(&mut image, Rect::new(30, 40 + row * 32 + 8, 220, 40 + row * 32 + 24), 16);
        }
        let mut areas = Areas::new(&image);
        let mut at = |x, y| areas.at(&image, image.bounds(), Point::new(x, y), 1.0);
        let list = at(250, 100).expect("no list");
        assert_eq!(at(250, 600), Some(list));
        assert!(list.top <= 8 && list.bottom >= 690 && list.width() >= 298, "{list:?}");
    }

    #[test]
    fn the_cells_of_a_grid_light_up_as_the_grid() {
        // A week of a calendar: cells parted by lines, too large not to be rooms each.
        let mut image = canvas(1200, 800, [255, 255, 255]);
        fill(&mut image, Rect::new(0, 0, 200, 800), [240, 240, 240]);
        for column in 0..=6 {
            fill(&mut image, Rect::new(240 + column * 150, 60, 241 + column * 150, 741), [220, 220, 220]);
        }
        for row in 0..=8 {
            fill(&mut image, Rect::new(240, 60 + row * 85, 1141, 61 + row * 85), [220, 220, 220]);
        }
        let mut areas = Areas::new(&image);
        let mut at = |x, y| areas.at(&image, image.bounds(), Point::new(x, y), 1.0);
        let grid = at(320, 100).expect("no grid");
        assert!(grid.left <= 241 && grid.right >= 1140 && grid.top <= 61 && grid.bottom >= 740, "{grid:?}");
        assert_eq!(at(1000, 700), Some(grid));
    }

    #[test]
    fn a_gradient_is_one_surface() {
        // A slide of a gradient across its whole height, on grey, beside a sidebar.
        let mut image = canvas(1200, 800, [230, 230, 230]);
        fill(&mut image, Rect::new(0, 0, 240, 800), [245, 245, 245]);
        fill(&mut image, Rect::new(240, 0, 241, 800), [200, 200, 200]);
        for y in 100..700 {
            let v = ((y - 100) / 4) as u8;
            fill(&mut image, Rect::new(320, y, 1120, y + 1), [40 + v, 60 + v, 200 - v / 2]);
        }
        let mut areas = Areas::new(&image);
        let mut at = |x, y| areas.at(&image, image.bounds(), Point::new(x, y), 1.0);
        let slide = at(700, 150).expect("no slide");
        assert_eq!(slide, Rect::new(320, 100, 1120, 700));
        assert_eq!(at(700, 650), Some(slide));
    }

    #[test]
    fn a_narrow_panel_beside_a_wide_room_is_a_room_of_its_own() {
        // A line parts a panel a sixth of the window wide from the wide room beside it, but for
        // a way round its end.
        let mut image = canvas(1200, 800, [255, 255, 255]);
        fill(&mut image, Rect::new(200, 0, 201, 740), [200, 200, 200]);
        let mut areas = Areas::new(&image);
        let mut at = |x, y| areas.at(&image, image.bounds(), Point::new(x, y), 1.0);
        let (panel, room) = (at(100, 300).expect("no panel"), at(800, 300).expect("no room"));
        assert!(panel.right <= 210 && room.left >= 190, "{panel:?} {room:?}");
    }

    #[test]
    fn the_middle_of_a_long_room_is_one_room() {
        // A terminal under a tab with a line under it: the line dents the middle of the room but
        // doesn't part it.
        let mut image = canvas(1600, 900, [245, 245, 245]);
        fill(&mut image, Rect::new(200, 300, 1600, 900), [30, 30, 30]);
        fill(&mut image, Rect::new(200, 300, 1600, 301), [80, 80, 80]);
        fill(&mut image, Rect::new(700, 340, 780, 342), [0, 120, 215]);
        text(&mut image, Rect::new(240, 400, 640, 800), 16);
        let mut areas = Areas::new(&image);
        let mut at = |x, y| areas.at(&image, image.bounds(), Point::new(x, y), 1.0);
        let room = at(1200, 600).expect("no room");
        assert!(room.left <= 201 && room.right == 1600 && room.top <= 302 && room.bottom == 900, "{room:?}");
        assert_eq!(at(300, 850), Some(room));
    }

    #[test]
    fn the_cuts_of_filling() {
        // Labels go by parts, in the order they begin, and diagonals only when they are asked for.
        #[rustfmt::skip]
        let mask = [
            true,  false, true,
            false, true,  true,
        ];
        assert_eq!(parts(&mask, 3, false).labels, vec![1, 0, 2, 0, 2, 2]);
        assert_eq!(parts(&mask, 3, true).labels, vec![1, 0, 1, 0, 1, 1]);
        assert_eq!(parts(&mask, 3, false).boxes[2], Rect::new(1, 0, 3, 2));
        assert_eq!(parts(&mask, 3, false).sizes[2], 3);
        // Only runs long enough are left.
        let row = [true, true, false, true, true, true];
        assert_eq!(runs(&row, 6, 3, true), vec![false, false, false, true, true, true]);
        assert_eq!(runs(&row, 3, 2, false), vec![true, true, false, true, true, false]);
        // A mask grows sideways only, if so asked.
        let dot = [false, false, true, false, false];
        assert_eq!(grown(&dot, 5, 1, 0), vec![false, true, true, true, false]);
        // The distance from the sides of a mask, past its sides too.
        assert_eq!(distances(&[true; 5], 5, false), vec![UNIT; 5]);
        assert_eq!(distances(&[true; 25], 5, false)[12], 3 * UNIT);
        assert_eq!(distances(&[true, true, false, true], 2, true)[1], UNIT * 14 / 10);
        // The median of what is around a pixel takes a stray pixel away.
        let mut pixels = vec![[10u8; 3]; 25 * 25];
        pixels[12 * 25 + 12] = [200, 200, 200];
        assert_eq!(smoothed(&pixels, 25, 2), vec![[10u8; 3]; 25 * 25]);
    }

    #[test]
    fn two_basins_part_by_the_width_of_the_narrower() {
        // A wide room and a narrow one with a way a pixel wide between them: the distance is
        // 8 wide across the narrower one, a door's depth over the way, and no less.
        let (w, h) = (120usize, 40usize);
        let mut mask = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                mask[y * w + x] = (x < 24 && y < 30) || x >= 28 || y == 15;
            }
        }
        let distance = distances(&mask, w, false);
        let labels = basins(&distance, w, LEAST_DOOR);
        assert_ne!(labels[15 * w + 8], labels[15 * w + 80]);
        // Not by a dent in the middle of one room.
        let mut mask = vec![true; w * h];
        mask[3 * w + 60] = false;
        let labels = basins(&distances(&mask, w, false), w, LEAST_DOOR);
        assert_eq!(labels[20 * w + 8], labels[20 * w + 110]);
    }

    /// Draws the areas that the mouse would find all over screenshots of windows into pictures to
    /// look at, the screenshot dimmed, every place of the mouse tinted in the colour of the frame
    /// that lights up there, and writes the area at every place into a text file beside each
    /// picture. The screenshots are PNG files in `SCREENSHOT_AREAS`, the pictures go into
    /// `SCREENSHOT_PREVIEW`, both folders, and `SCREENSHOT_SCALE` is the scale they were taken at,
    /// 1 by default: `SCREENSHOT_AREAS=<folder> SCREENSHOT_PREVIEW=<folder> cargo test -p ainitshot
    /// areas::tests::the_areas_of_screenshots_in_pictures -- --exact --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_areas_of_screenshots_in_pictures() {
        let from = PathBuf::from(std::env::var_os("SCREENSHOT_AREAS").expect("a folder of screenshots"));
        let to = PathBuf::from(std::env::var_os("SCREENSHOT_PREVIEW").expect("a folder for the pictures"));
        let scale: f32 = std::env::var("SCREENSHOT_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
        let mut files: Vec<PathBuf> = std::fs::read_dir(&from).unwrap().flatten().map(|e| e.path()).collect();
        files.retain(|path| path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")));
        files.sort();
        for path in files {
            let Ok(image) = Image::from_png(&std::fs::read(&path).unwrap()) else { continue };
            let started = std::time::Instant::now();
            let mut areas = Areas::new(&image);
            let whole = image.bounds();
            let step = (24.0 * scale) as usize;
            let (mut found, mut places, mut slowest): (Vec<Rect>, Vec<(i32, i32, usize)>, f32) = (Vec::new(), Vec::new(), 0.0);
            for y in (step / 2..image.height as usize).step_by(step) {
                for x in (step / 2..image.width as usize).step_by(step) {
                    let (x, y) = (x as i32, y as i32);
                    let point = std::time::Instant::now();
                    let area = areas.at(&image, whole, Point::new(x, y), scale);
                    slowest = slowest.max(point.elapsed().as_secs_f32());
                    if let Some(area) = area {
                        if !found.contains(&area) {
                            found.push(area);
                        }
                        places.push((x, y, found.iter().position(|a| *a == area).unwrap()));
                    }
                }
            }
            let seconds = started.elapsed().as_secs_f32();
            let picture = picture(&image, &found, &places, step as i32);
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            std::fs::write(to.join(&name), picture.to_png().unwrap()).unwrap();
            let lines: Vec<String> = places
                .iter()
                .map(|&(x, y, n)| {
                    let r = found[n];
                    format!("{x} {y} {} {} {} {}\n", r.left, r.top, r.right, r.bottom)
                })
                .collect();
            std::fs::write(to.join(&name).with_extension("txt"), lines.concat()).unwrap();
            println!("{name}: {} areas in {seconds:.2} s, the slowest place {:.0} ms", found.len(), slowest * 1000.0);
        }
    }

    /// The screenshot dimmed, the places of the mouse tinted and the frames of the areas drawn, of
    /// those lit from the most places last.
    fn picture(image: &Image, areas: &[Rect], places: &[(i32, i32, usize)], step: i32) -> Image {
        const COLORS: [[u8; 3]; 12] = [
            [230, 25, 75], [60, 180, 75], [0, 130, 200], [245, 130, 48], [145, 30, 180], [70, 240, 240],
            [240, 50, 230], [210, 245, 60], [250, 190, 212], [0, 128, 128], [220, 190, 255], [170, 110, 40],
        ];
        let mut picture = image.clone();
        for pixel in picture.pixels.chunks_exact_mut(4) {
            for c in &mut pixel[..3] {
                *c = (*c as u16 * 3 / 5 + 40) as u8;
            }
        }
        let bounds = picture.bounds();
        let tint = |picture: &mut Image, rect: Rect, color: [u8; 3], share: u16| {
            let rect = rect.intersect(bounds);
            for y in rect.top..rect.bottom {
                for x in rect.left..rect.right {
                    let i = ((y as u32 * picture.width + x as u32) * 4) as usize;
                    for (channel, c) in picture.pixels[i..i + 3].iter_mut().zip(color) {
                        *channel = ((*channel as u16 * (100 - share) + c as u16 * share) / 100) as u8;
                    }
                }
            }
        };
        let mut counts = vec![0; areas.len()];
        for &(x, y, n) in places {
            counts[n] += 1;
            let place = Rect::new(x - step / 2, y - step / 2, x + step / 2, y + step / 2);
            tint(&mut picture, place, COLORS[n % COLORS.len()], 27);
        }
        let mut order: Vec<usize> = (0..areas.len()).collect();
        order.sort_by_key(|&n| counts[n]);
        for n in order {
            let (r, color) = (areas[n], COLORS[n % COLORS.len()]);
            for side in [
                Rect::new(r.left, r.top, r.right, r.top + 3),
                Rect::new(r.left, r.bottom - 3, r.right, r.bottom),
                Rect::new(r.left, r.top, r.left + 3, r.bottom),
                Rect::new(r.right - 3, r.top, r.right, r.bottom),
            ] {
                tint(&mut picture, side, color, 100);
            }
        }
        picture
    }
}
