//! Areas of a window: the parts that a screenshot is most often taken of, a picture, a message, a
//! card, a panel or a paragraph, found in the pixels of the frozen screen. A window is built of
//! boxes: panels with surfaces of their own and lines between them, and on them blocks of content
//! with empty space around them. A box is cut along its edges, where one smooth surface meets
//! another across the whole box, or else along its gaps, the empty bands across it, the widest
//! first; every part is cut again the same way. The area under the mouse is the smallest part
//! around it that is big enough to be worth a screenshot: a panel, or the content of one, but
//! nothing inside a strip, such as a tab of a tab bar or a line of a paragraph.

use std::collections::HashMap;

use crate::geometry::{Point, Rect};
use crate::image::Image;

/// How much neighbouring pixels may differ in a colour channel and still be one smooth surface,
/// and how much two surfaces may and still be one.
const SAME: u8 = 6;
/// A pixel differs from the one on its left.
const ACROSS: u8 = 1;
/// A pixel differs from the one above it.
const DOWN: u8 = 2;
/// How much of a box an edge has to run along: all of it but rounded corners or a scroll bar,
/// not the margins around a box that is nearly as wide as the box around it.
const EDGE: f32 = 0.97;
/// How much of a row may differ from its neighbours on a smooth surface beside an edge, and how
/// many pixels of it at least: the rounded corners at its ends.
const SMOOTH: f32 = 1.0 / 32.0;
const CORNERS: u32 = 8;
/// How much of a row may differ from its neighbours in a gap: stray pixels.
const STRAY: f32 = 1.0 / 200.0;
/// Bands no thicker are the lines between parts, not parts.
const LINE: i32 = 3;
/// The gaps that cut a box at once are nearly as wide as its widest one.
const TIER: f32 = 0.6;
/// The smallest panel worth a screenshot at the scale of 100%, and the smallest block of content:
/// two lines of text are not one yet.
const PANEL_WIDTH: f32 = 48.0;
const PANEL_HEIGHT: f32 = 40.0;
const BLOCK_WIDTH: f32 = 64.0;
const BLOCK_HEIGHT: f32 = 56.0;
/// A band thinner than this at the scale of 100%, and eight times as long, is a strip: a toolbar,
/// a tab bar, a line of text.
const STRIP: f32 = 56.0;
/// An area this much of the window is the window. The window's frame is the chain of bands across
/// most of it, each no thicker than this at the scale of 100%, that goes on from its top and from
/// its bottom, each band no further than `CHAIN` from the one before at the scale of 100%, at any
/// depth of the window's parts: a title bar, tabs, toolbars, a ribbon, a ruler, a status bar; and
/// so down its sides.
const MOST: f32 = 0.9;
const FRAME: f32 = 160.0;
const ACROSS_MOST: f32 = 0.6;
const CHAIN: f32 = 8.0;
/// Parts of about one size, this many at least along a box, are the cells of a grid, a table or a
/// list, columns of days or rows of messages: pieces of it, not boxes of their own. Lines alone
/// between them make a grid of fewer.
const CELLS: usize = 4;
const LINED_CELLS: usize = 3;
const ONE_SIZE: f32 = 1.5;
/// How much the gaps between the rows of a list may differ from one another.
const LIST: f32 = 0.25;
/// A block with no more of its pixels differing from their neighbours, and at least this much of
/// it of the colour of its ground, is sparse, as text is, rather than dense, as a picture is: even
/// a smooth picture has colours of its own.
const SPARSE: f32 = 0.3;
const BARE: f32 = 0.5;
/// Sparse content gives way to the panel around it when it is this much of the panel, as the text
/// of a card or of a message is, or when the panel is no more than this much of the window, as a
/// sidebar or a terminal is: a paragraph is an area only in a large panel, such as an article's.
const MOSTLY: f32 = 0.5;
const MODERATE: f32 = 0.4;

/// A line or a step between the parts of a box: the row where it starts and the row where the
/// part after it does.
type Divider = (usize, usize);

/// A side of the window that its frame goes on from.
#[derive(Clone, Copy)]
enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    /// How far a part is from where the chain of bands from this side has come.
    fn gap(self, rect: Rect, edge: i32) -> i32 {
        match self {
            Side::Top => rect.top - edge,
            Side::Bottom => edge - rect.bottom,
            Side::Left => rect.left - edge,
            Side::Right => edge - rect.right,
        }
    }

    /// Where a band takes the chain on to.
    fn on(self, rect: Rect) -> i32 {
        match self {
            Side::Top => rect.bottom,
            Side::Bottom => rect.top,
            Side::Left => rect.right,
            Side::Right => rect.left,
        }
    }
}

/// What a part of a box is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// A panel: a surface of its own between edges, padding and all, or a box drawn with lines.
    Panel,
    /// A piece of the surface of the box around it, between edges: not an area of its own.
    Piece,
    /// A block of content, without the empty space around it.
    Block,
}

#[derive(Clone, Copy, Debug)]
struct Part {
    rect: Rect,
    kind: Kind,
    /// Whether a block's content is sparse, as text is.
    sparse: bool,
    /// The colour of the surface that the part's content is on.
    ground: [u8; 3],
    /// Whether nothing is on the part's surface.
    empty: bool,
}

/// The areas of the windows of a frozen screen, found as the mouse comes to them.
pub struct Areas {
    width: i32,
    height: i32,
    /// For every pixel, whether it differs from its neighbours on the left and above.
    bits: Vec<u8>,
    /// The boxes whose pixels have been looked at.
    looked: Vec<Rect>,
    /// The parts of every box cut so far.
    parts: HashMap<Rect, Vec<Part>>,
}

impl Areas {
    pub fn new(image: &Image) -> Self {
        let (width, height) = (image.width as i32, image.height as i32);
        Areas { width, height, bits: vec![0; (width * height) as usize], looked: Vec::new(), parts: HashMap::new() }
    }

    /// The area of the window under the point: the smallest part around it worth a screenshot,
    /// at the monitor's scale. Nothing if no part of the window is, but the window itself.
    pub fn at(&mut self, image: &Image, window: Rect, p: Point, scale: f32) -> Option<Rect> {
        let window = window.intersect(Rect::new(0, 0, self.width, self.height));
        if window.is_empty() || !window.contains(p) {
            return None;
        }
        self.look(image, window);
        let path = self.path(image, window, p);
        // On the window's frame, the area is the window.
        if self.frame(&path, window, scale) < path.len() {
            return None;
        }
        choose(&path, window, scale)
    }

    /// Where the path enters the window's frame, if it does: the first part on it that is a band
    /// of the chain of bands from an edge of the window.
    fn frame(&self, path: &[Part], window: Rect, scale: f32) -> usize {
        let reach = (CHAIN * scale) as i32;
        let across = |r: Rect| r.width() as f32 >= ACROSS_MOST * window.width() as f32 && (r.height() as f32 / scale) < FRAME;
        let down = |r: Rect| r.height() as f32 >= ACROSS_MOST * window.height() as f32 && (r.width() as f32 / scale) < FRAME;
        // A column down a side of the window too wide to be a band of its frame is a dock: what is
        // in it is the dock's, not the frame's.
        let dock = |r: Rect| r.height() as f32 >= ACROSS_MOST * window.height() as f32 && 2 * r.width() < window.width();
        // Where the chains from the top, the bottom, the left and the right have come so far, and
        // whether a dock has ended one.
        let mut edges = [
            (Side::Top, window.top, false),
            (Side::Bottom, window.bottom, false),
            (Side::Left, window.left, false),
            (Side::Right, window.right, false),
        ];
        for (n, pair) in path.windows(2).enumerate() {
            let (outer, inner) = (pair[0], pair[1]);
            let Some(parts) = self.parts.get(&outer.rect) else { break };
            for (side, edge, ended) in &mut edges {
                let sides = matches!(side, Side::Left | Side::Right);
                let band = |r: Rect| if sides { down(r) } else { across(r) };
                let mut sorted: Vec<Rect> = parts.iter().map(|part| part.rect).collect();
                sorted.sort_by_key(|r| side.gap(*r, *edge));
                for r in sorted {
                    if *ended || side.gap(r, *edge) > reach {
                        break;
                    }
                    if !band(r) {
                        *ended = sides && dock(r);
                        break;
                    }
                    if r == inner.rect {
                        return n + 1;
                    }
                    *edge = side.on(r);
                }
            }
        }
        path.len()
    }

    /// The parts around the point, from the window down to the smallest.
    fn path(&mut self, image: &Image, window: Rect, p: Point) -> Vec<Part> {
        let ground = background(image, window);
        let mut path = vec![Part { rect: window, kind: Kind::Panel, sparse: false, ground, empty: false }];
        while path.len() < 48 {
            let last = path[path.len() - 1];
            let inside = self.parts_of(image, last).iter().find(|part| part.rect.contains(p) && part.rect != last.rect);
            let Some(part) = inside.copied() else { break };
            path.push(part);
        }
        path
    }

    fn parts_of(&mut self, image: &Image, part: Part) -> &[Part] {
        if !self.parts.contains_key(&part.rect) {
            let parts = self.cut(image, part);
            self.parts.insert(part.rect, parts);
        }
        &self.parts[&part.rect]
    }

    /// Notes where the pixels of the box differ from their neighbours, once.
    fn look(&mut self, image: &Image, rect: Rect) {
        if self.looked.iter().any(|looked| looked.intersect(rect) == rect) {
            return;
        }
        let (width, pixels) = (self.width as usize, &image.pixels);
        let differ = |a: usize, b: usize| (0..3).any(|c| pixels[a + c].abs_diff(pixels[b + c]) > SAME);
        for y in rect.top as usize..rect.bottom as usize {
            for x in rect.left as usize..rect.right as usize {
                let i = (y * width + x) * 4;
                let mut bits = 0;
                if x > 0 && differ(i - 4, i) {
                    bits |= ACROSS;
                }
                if y > 0 && differ(i - width * 4, i) {
                    bits |= DOWN;
                }
                self.bits[y * width + x] = bits;
            }
        }
        self.looked.push(rect);
    }

    /// Cuts a part into its parts: along its edges if it has any, or else along its widest gaps;
    /// without gaps, its content is its only part, if the content is smaller than it. Edges all
    /// along the part cut it before those that stop short of a band across it, and that band is
    /// cut off before them. Edges that stop short of both ends are those of a box inside it with
    /// margins around it, nearly as wide as it, a slide in the space around it: gaps cut the part
    /// before them.
    fn cut(&self, image: &Image, part: Part) -> Vec<Part> {
        let (rect, ground) = (part.rect, part.ground);
        let (w, h) = (rect.width() as usize, rect.height() as usize);
        if w < 8 || h < 8 {
            return Vec::new();
        }
        let profile = self.profile(rect);
        let rows = self.edges(rect, &profile, true);
        let columns = self.edges(rect, &profile, false);
        let (row_blank, column_blank) = empty(image, rect, &profile, ground);
        // The box's own edges, across it before down it; those that stop short of both its ends
        // only after its gaps.
        for (edges, across) in [(&rows, true), (&columns, false)] {
            let own: Vec<&Edge> = edges.iter().filter(|e| e.ends() != (false, false)).collect();
            if own.is_empty() {
                continue;
            }
            // Edges that all stop short of an end of the box meet a band across it there, as
            // panels side by side meet a menu bar over them: the band is cut off first, where
            // nothing goes on from it into the rest.
            if own.iter().all(|e| e.short.0 > CORNERS || e.short.1 > CORNERS)
                && let Some(at) = junction(&profile, &own, across, if across { &column_blank } else { &row_blank })
            {
                let piece = |rect: Rect| Part { rect, kind: Kind::Piece, sparse: false, ground, empty: false };
                let at = at as i32;
                return if across {
                    vec![piece(Rect::new(rect.left, rect.top, rect.left + at, rect.bottom)), piece(Rect::new(rect.left + at, rect.top, rect.right, rect.bottom))]
                } else {
                    vec![piece(Rect::new(rect.left, rect.top, rect.right, rect.top + at)), piece(Rect::new(rect.left, rect.top + at, rect.right, rect.bottom))]
                };
            }
            let cuts: Vec<usize> = own.iter().map(|e| e.at).collect();
            return self.panels(image, part, &profile, &cuts, across);
        }
        let (Some(row_span), Some(column_span)) = (span(&row_blank), span(&column_blank)) else { return Vec::new() };
        let row_gaps = gaps(&row_blank, row_span);
        let column_gaps = gaps(&column_blank, column_span);
        let widest = |gaps: &[(usize, usize)]| gaps.iter().map(|(start, end)| end - start).max().unwrap_or(0);
        let (row_widest, column_widest) = (widest(&row_gaps), widest(&column_gaps));
        if row_widest == 0 && column_widest == 0 {
            for (edges, across) in [(&rows, true), (&columns, false)] {
                if !edges.is_empty() {
                    let cuts: Vec<usize> = edges.iter().map(|e| e.at).collect();
                    return self.panels(image, part, &profile, &cuts, across);
                }
            }
            let content = self.content(image, rect, ground);
            return content.into_iter().filter(|content| content.rect != rect).collect();
        }
        // The rows of a list or of a table go across it before its columns, however wide the
        // space between the columns: a piece of a column is nothing to take.
        let (row_list, column_list) = (list(&row_gaps, row_span), list(&column_gaps, column_span));
        let across = if row_list != column_list { row_list } else { row_widest >= column_widest };
        let (gaps, span, widest) =
            if across { (row_gaps, row_span, row_widest) } else { (column_gaps, column_span, column_widest) };
        let listed = (across && row_list) || (!across && column_list);
        if listed {
            // The list itself is a part of the box, with the empty space around it, and its rows
            // are the parts of the list.
            let list = Rect::new(rect.left + column_span.0 as i32, rect.top + row_span.0 as i32, rect.left + column_span.1 as i32, rect.top + row_span.1 as i32);
            if !near(rect, list) {
                return self.content(image, rect, ground).into_iter().collect();
            }
        }
        let least = if listed { 1 } else { ((widest as f32 * TIER).ceil() as usize).max(1) };
        let mut bands = Vec::new();
        let mut start = span.0;
        for &(gap_start, gap_end) in gaps.iter().filter(|(start, end)| end - start >= least) {
            bands.push((start, gap_start));
            start = gap_end;
        }
        bands.push((start, span.1));
        bands
            .into_iter()
            .filter_map(|(start, end)| {
                let band = if across {
                    Rect::new(rect.left, rect.top + start as i32, rect.right, rect.top + end as i32)
                } else {
                    Rect::new(rect.left + start as i32, rect.top, rect.left + end as i32, rect.bottom)
                };
                self.content(image, band, ground)
            })
            .collect()
    }

    /// How the pixels of a box differ, row by row and column by column: inside each row and from
    /// the row above it, inside each column and from the column on its left.
    fn profile(&self, rect: Rect) -> Profile {
        let (w, h) = (rect.width() as usize, rect.height() as usize);
        let mut profile = Profile {
            row_detail: vec![0; h],
            row_edge: vec![0; h],
            column_detail: vec![0; w],
            column_edge: vec![0; w],
        };
        for i in 0..h {
            let start = (rect.top as usize + i) * self.width as usize + rect.left as usize;
            for (j, bits) in self.bits[start..start + w].iter().enumerate() {
                if j > 0 && bits & ACROSS != 0 {
                    profile.row_detail[i] += 1;
                    profile.column_edge[j] += 1;
                }
                if i > 0 && bits & DOWN != 0 {
                    profile.row_edge[i] += 1;
                    profile.column_detail[j] += 1;
                }
            }
        }
        profile
    }

    /// The edges across a box: rows that differ from the one before nearly all along it, with
    /// smooth surfaces on both sides, or columns if not `across`; and how far each stops short of
    /// the box's start and of its end.
    fn edges(&self, rect: Rect, profile: &Profile, across: bool) -> Vec<Edge> {
        let (edge, detail, length, count) = if across {
            (&profile.row_edge, &profile.row_detail, rect.width(), rect.height())
        } else {
            (&profile.column_edge, &profile.column_detail, rect.height(), rect.width())
        };
        let along = (EDGE * length as f32) as u32;
        let smooth = ((SMOOTH * length as f32) as u32).max(CORNERS);
        (1..count as usize)
            .filter(|&i| edge[i] >= along && detail[i - 1] <= smooth && detail[i] <= smooth)
            .map(|i| {
                let differs = |k: i32| {
                    let (x, y, bit) =
                        if across { (rect.left + k, rect.top + i as i32, DOWN) } else { (rect.left + i as i32, rect.top + k, ACROSS) };
                    self.bits[(y * self.width + x) as usize] & bit != 0
                };
                Edge { at: i, short: (solid(differs, 0..length), solid(differs, (0..length).rev())) }
            })
            .collect()
    }

    /// The parts between the edges across a box, on its ground. A single edge is a step from one
    /// surface to another, and so is a line between two surfaces of different colours; a line with
    /// the same surface on both sides only divides it. A part beside a step with a surface of its
    /// own is a panel, padding, border and all; beside a step on the box's ground it is a piece of
    /// the box. A part between two lines is a box drawn with them, a panel with its lines. Any
    /// other part is a piece of the box's surface.
    fn panels(&self, image: &Image, around: Part, profile: &Profile, cuts: &[usize], across: bool) -> Vec<Part> {
        let (rect, ground) = (around.rect, around.ground);
        let length = if across { rect.height() } else { rect.width() } as usize;
        // The lines and the steps between the parts: where they start and where the part after
        // them does. A line at either end of the box is the box's own.
        let mut dividers: Vec<Divider> = Vec::new();
        for &cut in cuts {
            match dividers.last_mut() {
                Some(last) if cut - last.1 <= LINE as usize => last.1 = cut,
                _ => dividers.push((cut, cut)),
            }
        }
        if let Some(first) = dividers.first_mut()
            && first.0 <= LINE as usize
        {
            first.0 = 0;
        }
        if let Some(last) = dividers.last_mut()
            && length - last.1 <= LINE as usize
        {
            last.1 = length;
        }
        let step = |&(start, end): &Divider| {
            start > 0 && end < length && (start == end || !same_surface(image, rect, across, start - 1, end))
        };
        let band = |start: usize, end: usize| {
            let (start, end) = (start as i32, end as i32);
            if across {
                Rect::new(rect.left, rect.top + start, rect.right, rect.top + end)
            } else {
                Rect::new(rect.left + start, rect.top, rect.left + end, rect.bottom)
            }
        };
        // Nothing differs across or along the rows of an empty part, but at its ends.
        let (detail, edge, along) = if across {
            (&profile.row_detail, &profile.row_edge, rect.width() as usize)
        } else {
            (&profile.column_detail, &profile.column_edge, rect.height() as usize)
        };
        let stray = (STRAY * along as f32) as u32;
        let empty = |start: usize, end: usize| (start..end).all(|i| detail[i] <= stray && (i == start || edge[i] <= stray));
        // The parts between lines alone, if they are the cells of a grid.
        let segments: Vec<(Option<Divider>, Option<Divider>)> = (0..=dividers.len())
            .map(|k| (k.checked_sub(1).map(|k| dividers[k]), dividers.get(k).copied()))
            .filter(|(before, after)| {
                let (start, end) = (before.map_or(0, |d| d.1), after.map_or(length, |d| d.0));
                end as i32 - start as i32 > LINE
            })
            .collect();
        let lengths: Vec<usize> = segments
            .iter()
            .map(|(before, after)| after.map_or(length, |d| d.0) - before.map_or(0, |d| d.1))
            .collect();
        let mut sorted = lengths.clone();
        sorted.sort_unstable();
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0) as f32;
        let cell = |n: usize| {
            let size = lengths[n] as f32;
            size <= ONE_SIZE * median && size * ONE_SIZE >= median
        };
        let lined = |n: usize| {
            let (before, after) = segments[n];
            before.is_some() && after.is_some() && !before.as_ref().is_some_and(step) && !after.as_ref().is_some_and(step)
        };
        let cells = (0..segments.len()).filter(|&n| cell(n)).count();
        let lined_cells = (0..segments.len()).filter(|&n| cell(n) && lined(n)).count();
        let grid = cells >= CELLS || lined_cells >= LINED_CELLS;
        let mut parts = Vec::new();
        for (n, &(before, after)) in segments.iter().enumerate() {
            let (start, end) = (before.map_or(0, |d| d.1), after.map_or(length, |d| d.0));
            // The part's surface, on the smooth row beside a divider.
            let beside = if before.is_some() { start } else { end - 1 };
            let surface = row_color(image, rect, across, beside);
            let own = !alike(surface, ground, SAME);
            let empty = empty(start, end);
            let part = |rect: Rect, kind: Kind, ground: [u8; 3]| Part { rect, kind, sparse: false, ground, empty };
            if grid && cell(n) {
                parts.push(part(band(start, end), Kind::Piece, surface));
            } else if before.as_ref().is_some_and(step) || after.as_ref().is_some_and(step) {
                if own {
                    // A line between two surfaces is the border of the one on the ground.
                    let from = before.map_or(start, |d| d.0);
                    let to = after.map_or(end, |d| d.1);
                    parts.push(part(band(from, to), Kind::Panel, surface));
                } else {
                    parts.push(part(band(start, end), Kind::Piece, ground));
                }
            } else if let (Some(before), Some(after)) = (before, after) {
                // The box's own lines all round leave its inside to be cut, the same kind of part;
                // a cell of a grid is a piece of it.
                if before.0 == 0 && after.1 == length {
                    parts.push(Part { rect: band(start, end), ground: surface, empty, ..around });
                } else {
                    parts.push(part(band(before.0, after.1), Kind::Panel, surface));
                }
            } else {
                parts.push(part(band(start, end), Kind::Piece, ground));
            }
        }
        parts
    }

    /// The block of content in the box, on its ground, without the empty space around it; nothing
    /// if the box is all empty.
    fn content(&self, image: &Image, rect: Rect, ground: [u8; 3]) -> Option<Part> {
        let profile = self.profile(rect);
        let (row_blank, column_blank) = empty(image, rect, &profile, ground);
        let (rows, columns) = (span(&row_blank)?, span(&column_blank)?);
        let content = Rect::new(
            rect.left + columns.0 as i32,
            rect.top + rows.0 as i32,
            rect.left + columns.1 as i32,
            rect.top + rows.1 as i32,
        );
        let differing: u32 = profile.row_detail.iter().chain(&profile.column_detail).sum();
        let pixels = 2.0 * content.width() as f32 * content.height() as f32;
        let sparse = (differing as f32) < SPARSE * pixels && bare(image, content, ground) >= BARE;
        Some(Part { rect: content, kind: Kind::Block, sparse, ground, empty: false })
    }
}

/// An edge across a box, or down it: the row or the column where it is, and how far it stops
/// short of the start of the box and of its end.
struct Edge {
    at: usize,
    short: (u32, u32),
}

impl Edge {
    /// Whether the edge comes to the start of the box, and to its end, but for a line's width.
    fn ends(&self) -> (bool, bool) {
        (self.short.0 <= LINE as u32, self.short.1 <= LINE as u32)
    }
}

/// How the pixels of a box differ: see [`Areas::profile`].
struct Profile {
    row_detail: Vec<u32>,
    row_edge: Vec<u32>,
    column_detail: Vec<u32>,
    column_edge: Vec<u32>,
}

/// The colour of a pixel.
fn color(image: &Image, x: i32, y: i32) -> [u8; 3] {
    let i = ((y as u32 * image.width + x as u32) * 4) as usize;
    [image.pixels[i], image.pixels[i + 1], image.pixels[i + 2]]
}

/// Whether two colours are of one surface, within `by` in every channel.
fn alike(a: [u8; 3], b: [u8; 3], by: u8) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= by)
}

/// The colour of a smooth row of a box, or of a column: the middle of three samples along it.
fn row_color(image: &Image, rect: Rect, across: bool, index: usize) -> [u8; 3] {
    let index = index as i32;
    let samples = [1, 2, 3].map(|k| {
        if across {
            color(image, rect.left + rect.width() * k / 4, rect.top + index)
        } else {
            color(image, rect.left + index, rect.top + rect.height() * k / 4)
        }
    });
    let mut middle = [0u8; 3];
    for (c, channel) in middle.iter_mut().enumerate() {
        let mut values = samples.map(|s| s[c]);
        values.sort_unstable();
        *channel = values[1];
    }
    middle
}

/// Whether two rows of a box, or two columns, are of the same surfaces, pixel by pixel for the
/// most part: the two sides of a line that only divides one surface.
fn same_surface(image: &Image, rect: Rect, across: bool, a: usize, b: usize) -> bool {
    let (a, b) = (a as i32, b as i32);
    let pairs: Vec<([u8; 3], [u8; 3])> = if across {
        (rect.left..rect.right).map(|x| (color(image, x, rect.top + a), color(image, x, rect.top + b))).collect()
    } else {
        (rect.top..rect.bottom).map(|y| (color(image, rect.left + a, y), color(image, rect.left + b, y))).collect()
    };
    let same = pairs.iter().filter(|(a, b)| alike(*a, *b, SAME)).count();
    2 * same >= pairs.len()
}

/// The colour of most of a box: of the most pixels on a grid over it, by colours a little apart.
fn background(image: &Image, rect: Rect) -> [u8; 3] {
    let step = ((size(rect) / 4096.0).sqrt().ceil() as usize).max(1);
    let mut counts = vec![0u16; 1 << 15];
    let mut best = (0u16, [0u8; 3]);
    for y in (rect.top..rect.bottom).step_by(step) {
        for x in (rect.left..rect.right).step_by(step) {
            let c = color(image, x, y);
            let bucket = (c[0] as usize >> 3) << 10 | (c[1] as usize >> 3) << 5 | c[2] as usize >> 3;
            counts[bucket] = counts[bucket].saturating_add(1);
            if counts[bucket] > best.0 {
                best = (counts[bucket], c);
            }
        }
    }
    best.1
}

/// How much of a box is of the colour of the ground, on a grid over it.
fn bare(image: &Image, rect: Rect, ground: [u8; 3]) -> f32 {
    let step = ((size(rect) / 1024.0).sqrt().ceil() as usize).max(1);
    let (mut all, mut bare) = (0u32, 0u32);
    for y in (rect.top..rect.bottom).step_by(step) {
        for x in (rect.left..rect.right).step_by(step) {
            all += 1;
            if alike(color(image, x, y), ground, 2 * SAME) {
                bare += 1;
            }
        }
    }
    bare as f32 / all.max(1) as f32
}

/// Which rows of a box are empty, and which columns: nothing differs along them but stray pixels,
/// and they are of the colour of the ground the content is on all along, the empty space around
/// it rather than a line, a band of a colour of its own or the side of a panel.
fn empty(image: &Image, rect: Rect, profile: &Profile, ground: [u8; 3]) -> (Vec<bool>, Vec<bool>) {
    let (w, h) = (rect.width(), rect.height());
    let rows = blank(&profile.row_detail, w as usize)
        .into_iter()
        .enumerate()
        .map(|(i, blank)| {
            let y = rect.top + i as i32;
            blank && (1..8).all(|k| alike(color(image, rect.left + w * k / 8, y), ground, 2 * SAME))
        })
        .collect();
    let columns = blank(&profile.column_detail, h as usize)
        .into_iter()
        .enumerate()
        .map(|(j, blank)| {
            let x = rect.left + j as i32;
            blank && (1..8).all(|k| alike(color(image, x, rect.top + h * k / 8), ground, 2 * SAME))
        })
        .collect();
    (rows, columns)
}

/// How far along a row of pixels, or a column, the edge on it begins: at the first of `CORNERS`
/// differing pixels in a row, past rounded corners and stray pixels. Never, if there are none.
fn solid(differs: impl Fn(i32) -> bool, along: impl Iterator<Item = i32>) -> u32 {
    let mut run = 0;
    for (n, k) in along.enumerate() {
        run = if differs(k) { run + 1 } else { 0 };
        if run == CORNERS {
            return n as u32 + 1 - CORNERS;
        }
    }
    u32::MAX
}

/// Where a band across a box meets the edges down it that stop short of it, if nothing goes on
/// from the band into the rest there: the row where the rest begins, or the column, if the edges
/// are across the box. A line at the start of the rest stays with it: the band is cut off along
/// the empty space next to the line, if there is any. `blank` tells the empty rows, or columns.
fn junction(profile: &Profile, edges: &[&Edge], across: bool, blank: &[bool]) -> Option<usize> {
    let (detail, along) =
        if across { (&profile.column_detail, profile.row_detail.len()) } else { (&profile.row_detail, profile.column_detail.len()) };
    let length = detail.len();
    // A line down the box differs from its sides in every row of the rest.
    let smooth = ((SMOOTH * along as f32) as u32).max(CORNERS) + 2 * edges.len() as u32;
    // Where the band ends, and whether it is before that or after.
    let mut ends: Vec<(usize, bool)> = Vec::new();
    for edge in edges {
        if edge.short.0 > CORNERS {
            ends.push((edge.short.0 as usize, true));
        }
        if edge.short.1 > CORNERS {
            ends.push((length.saturating_sub(edge.short.1 as usize), false));
        }
    }
    ends.sort_unstable();
    ends.dedup();
    let (end, before) =
        ends.into_iter().find(|&(end, _)| end > 0 && end < length && detail[end - 1] <= smooth && detail[end] <= smooth)?;
    let shifted = (0..=LINE as usize + 1).filter_map(|k| if before { end.checked_sub(k) } else { Some(end + k) });
    Some(shifted.take_while(|&at| at > 0 && at < length).find(|&at| blank[if before { at - 1 } else { at }]).unwrap_or(end))
}

/// Which rows of a box have nothing differing along them but stray pixels.
fn blank(detail: &[u32], length: usize) -> Vec<bool> {
    let stray = (STRAY * length as f32) as u32;
    detail.iter().map(|&d| d <= stray).collect()
}

/// From the first row with something in it to just after the last; nothing if all are empty.
fn span(blank: &[bool]) -> Option<(usize, usize)> {
    let first = blank.iter().position(|b| !b)?;
    let last = blank.iter().rposition(|b| !b)?;
    Some((first, last + 1))
}

/// Whether the gaps across a box, inside the span of its content, are those between the rows of a
/// list or a table: `CELLS` rows at least, most of about one size, with gaps of about one width
/// between them, some narrower, as where a row is lit up, but none much wider, as between the
/// paragraphs of a text.
fn list(gaps: &[(usize, usize)], span: (usize, usize)) -> bool {
    if gaps.len() + 1 < CELLS {
        return false;
    }
    let mostly = |mut sizes: Vec<usize>, by: f32| {
        sizes.sort_unstable();
        let median = sizes[sizes.len() / 2] as f32;
        let alike = sizes.iter().filter(|&&size| (size as f32 - median).abs() <= by * median).count();
        (4 * alike >= 3 * sizes.len(), sizes[sizes.len() - 1] as f32 <= ONE_SIZE * median)
    };
    let widths: Vec<usize> = gaps.iter().map(|(start, end)| end - start).collect();
    let mut rows = Vec::new();
    let mut start = span.0;
    for &(gap_start, gap_end) in gaps {
        rows.push(gap_start - start);
        start = gap_end;
    }
    rows.push(span.1 - start);
    let (gaps_alike, none_wider) = mostly(widths, LIST);
    let (rows_alike, _) = mostly(rows, ONE_SIZE - 1.0);
    gaps_alike && none_wider && rows_alike
}

/// The runs of empty rows inside the span, as (start, end).
fn gaps(blank: &[bool], span: (usize, usize)) -> Vec<(usize, usize)> {
    let mut gaps = Vec::new();
    let mut start = None;
    for (i, &empty) in blank.iter().enumerate().take(span.1).skip(span.0) {
        match (empty, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                gaps.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    gaps
}

/// The area among the parts around the point, from the window down: the smallest panel or block
/// worth a screenshot, but nothing inside a strip. A part barely smaller than the one around it
/// is that one, as a box is its inside without its lines; sparse content gives way to the panel
/// around it when it is most of the panel or the panel is of a moderate size. Nothing if the area
/// would be the window, or most of it.
fn choose(path: &[Part], window: Rect, scale: f32) -> Option<Rect> {
    let mut levels: Vec<Part> = Vec::new();
    for part in path {
        match levels.last_mut() {
            Some(last) if near(last.rect, part.rect) => {
                if part.kind == Kind::Panel {
                    last.kind = Kind::Panel;
                }
            }
            _ => levels.push(*part),
        }
    }
    let open =levels.iter().position(|part| strip(part.rect, window, scale)).unwrap_or(levels.len());
    let levels = &levels[..open];
    let deepest = (1..levels.len()).rev().find(|&i| big(&levels[i], scale))?;
    let mut area = levels[deepest];
    if area.kind == Kind::Block && area.sparse {
        let around = levels[1..deepest].iter().rev().find(|part| part.kind == Kind::Panel);
        if let Some(panel) = around
            && (size(area.rect) >= MOSTLY * size(panel.rect) || size(panel.rect) <= MODERATE * size(window))
        {
            area = *panel;
        }
    }
    (size(area.rect) < MOST * size(window)).then_some(area.rect)
}

/// Whether a part is barely smaller than the one around it, by lines at most on every side.
fn near(outer: Rect, inner: Rect) -> bool {
    let close = LINE + 1;
    inner.left - outer.left <= close
        && inner.top - outer.top <= close
        && outer.right - inner.right <= close
        && outer.bottom - inner.bottom <= close
}

fn size(rect: Rect) -> f32 {
    rect.width() as f32 * rect.height() as f32
}

/// Whether a part is a thin band at the monitor's scale: a toolbar, a tab bar, a line of text.
fn strip(rect: Rect, _window: Rect, scale: f32) -> bool {
    let (w, h) = (rect.width() as f32 / scale, rect.height() as f32 / scale);
    (h < STRIP && w >= 8.0 * h) || (w < STRIP && h >= 8.0 * w)
}

/// Whether a part is big enough to be an area at the monitor's scale; an empty part, or a piece of
/// a surface, never is one of its own.
fn big(part: &Part, scale: f32) -> bool {
    let (w, h) = (part.rect.width() as f32 / scale, part.rect.height() as f32 / scale);
    if part.empty {
        return false;
    }
    match part.kind {
        Kind::Panel => w >= PANEL_WIDTH && h >= PANEL_HEIGHT,
        Kind::Block => w >= BLOCK_WIDTH && h >= BLOCK_HEIGHT,
        Kind::Piece => false,
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

    /// Prints the parts around points of a screenshot, from the window down, and the area chosen:
    /// `SCREENSHOT_AREAS_FILE=<file.png> SCREENSHOT_AREAS_POINTS="x,y x,y" cargo test -p ainitshot
    /// areas::tests::the_parts_around_points -- --exact --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_parts_around_points() {
        let file = std::env::var_os("SCREENSHOT_AREAS_FILE").expect("a screenshot");
        let image = Image::from_png(&std::fs::read(file).unwrap()).unwrap();
        let points = std::env::var("SCREENSHOT_AREAS_POINTS").expect("points");
        let mut areas = Areas::new(&image);
        let whole = image.bounds();
        areas.look(&image, whole);
        for point in points.split_whitespace() {
            let (x, y) = point.split_once(',').unwrap();
            let p = Point::new(x.parse().unwrap(), y.parse().unwrap());
            println!("== {p:?}: {:?}", areas.at(&image, whole, p, 1.0));
            let path = areas.path(&image, whole, p);
            for part in &path {
                let r = part.rect;
                let g = part.ground;
                println!(
                    "   {:?} {}..{} x {}..{} sparse {} ground {:02x}{:02x}{:02x}",
                    part.kind, r.left, r.right, r.top, r.bottom, part.sparse, g[2], g[1], g[0]
                );
            }
            // What the smallest part was cut into, without the point in any of them.
            for part in areas.parts.get(&path[path.len() - 1].rect).into_iter().flatten() {
                let r = part.rect;
                println!("      {:?} {}..{} x {}..{}", part.kind, r.left, r.right, r.top, r.bottom);
            }
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
        assert!(paragraph.top >= 165 && paragraph.bottom <= 315 && paragraph.left >= 295, "{paragraph:?}");
        // The sidebar takes in the line on its side, its own border.
        let sidebar = at(&mut areas, 100, 600).expect("no sidebar");
        assert_eq!(sidebar, Rect::new(0, 81, 261, 800));
        // The toolbar is a strip: the window, nothing of its own.
        assert_eq!(at(&mut areas, 600, 50), None);
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

    /// Draws the areas that the mouse would find all over screenshots of windows, each in a
    /// colour of its own over the screenshot dimmed, into pictures to look at. The screenshots are
    /// PNG files in `SCREENSHOT_AREAS`, the pictures go into `SCREENSHOT_PREVIEW`, both folders,
    /// and `SCREENSHOT_SCALE` is the scale they were taken at, 1 by default:
    /// `SCREENSHOT_AREAS=<folder> SCREENSHOT_PREVIEW=<folder> cargo test -p ainitshot
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
            let mut found: Vec<Rect> = Vec::new();
            for y in (step / 2..image.height as usize).step_by(step) {
                for x in (step / 2..image.width as usize).step_by(step) {
                    if let Some(area) = areas.at(&image, whole, Point::new(x as i32, y as i32), scale)
                        && !found.contains(&area)
                    {
                        found.push(area);
                    }
                }
            }
            let seconds = started.elapsed().as_secs_f32();
            let picture = picture(&image, &found);
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            std::fs::write(to.join(&name), picture.to_png().unwrap()).unwrap();
            println!("{name}: {} areas in {seconds:.2} s", found.len());
            found.sort_by_key(|r| (r.top, r.left, r.bottom, r.right));
            for r in &found {
                println!("   {}..{} x {}..{}", r.left, r.right, r.top, r.bottom);
            }
        }
    }

    /// The screenshot dimmed, with every area outlined and lightly filled in a colour of its own.
    fn picture(image: &Image, areas: &[Rect]) -> Image {
        const COLORS: [[u8; 3]; 8] = [
            [230, 25, 75], [60, 180, 75], [0, 130, 200], [245, 130, 48],
            [145, 30, 180], [70, 240, 240], [240, 50, 230], [210, 245, 60],
        ];
        let mut picture = image.clone();
        for pixel in picture.pixels.chunks_exact_mut(4) {
            for c in &mut pixel[..3] {
                *c = (*c as u16 * 3 / 5 + 40) as u8;
            }
        }
        let bounds = picture.bounds();
        for (n, area) in areas.iter().enumerate() {
            let color = COLORS[n % COLORS.len()];
            let tint = |picture: &mut Image, x: i32, y: i32, share: u16| {
                let i = ((y as u32 * picture.width + x as u32) * 4) as usize;
                for (channel, c) in picture.pixels[i..i + 3].iter_mut().zip(color) {
                    *channel = ((*channel as u16 * (100 - share) + c as u16 * share) / 100) as u8;
                }
            };
            let area = area.intersect(bounds);
            for y in area.top..area.bottom {
                for x in area.left..area.right {
                    let edge = x < area.left + 2 || x >= area.right - 2 || y < area.top + 2 || y >= area.bottom - 2;
                    tint(&mut picture, x, y, if edge { 100 } else { 12 });
                }
            }
        }
        picture
    }
}
