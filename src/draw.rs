//! How the frozen screen looks: dimmed everywhere except the selection, with the annotations on
//! it, a thin frame with round handles around it, its size next to it and the toolbar. Everything
//! is composed in a frame of the overlay's size, and only the part that changed is redrawn.
//!
//! The look is minimal and modern (see the requirements): nothing but the selection and what
//! belongs to it, smooth shapes, quiet colours, the toolbar in the style of Windows 11.
//!
//! Pixels here go in the order blue, green, red, opacity, as Windows keeps them. The library
//! writes red first, so every colour is given to it with red and blue swapped (see `solid`).

use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PixmapMut, PixmapPaint, Stroke, StrokeDash, Transform,
};

use crate::annotation::{Annotation, Coverage, Label, Shape, Tool, arrow_head, shadow};
use crate::color::Rgba;
use crate::geometry::{Point, PointF, Rect};
use crate::icons::{self, Part};
use crate::image::Image;
use crate::settings::Icon;
use crate::toolbar::{Button, Palette, Toolbar};

/// How much brightness the screen keeps outside the selection, out of 256.
const DIM: u16 = 120;

const ACCENT: Rgba = [47, 111, 235, 255];
const WHITE: Rgba = [255, 255, 255, 255];
const FRAME: Rgba = [255, 255, 255, 235];
const HANDLE_RING: Rgba = [0, 0, 0, 80];
const PANEL: Rgba = [32, 32, 32, 246];
const PANEL_EDGE: Rgba = [255, 255, 255, 22];
const PILL: Rgba = [28, 28, 28, 232];
const HOVER: Rgba = [255, 255, 255, 26];
const INK: Rgba = [236, 236, 236, 255];
const INK_OFF: Rgba = [236, 236, 236, 80];
const SEPARATOR: Rgba = [255, 255, 255, 30];
pub const TEXT: Rgba = [242, 242, 242, 255];

// Sizes in pixels at the scale of 100%.
const HANDLE_RADIUS: f32 = 4.0;
const PICKED_RADIUS: f32 = 4.5;
const LABEL_FONT: f32 = 12.0;
const PILL_PADDING_X: f32 = 8.0;
const PILL_PADDING_Y: f32 = 3.0;
const LABEL_GAP: f32 = 6.0;
const PANEL_RADIUS: f32 = 10.0;
const BUTTON_RADIUS: f32 = 6.0;
const ICON: f32 = 20.0;
/// How far from an edge of the selection the mouse still takes it.
const EDGE_REACH: f32 = 6.0;

/// The picture the overlay is composed in, the same size as the frozen screen.
pub struct Frame<'a> {
    pub pixels: &'a mut [u8],
    pub width: u32,
    pub height: u32,
}

impl Frame<'_> {
    fn pixmap(&mut self) -> Option<PixmapMut<'_>> {
        PixmapMut::from_bytes(self.pixels, self.width, self.height)
    }
}

/// Text to be drawn by the system over what is composed here, centred in `rect`.
pub struct Text {
    pub text: String,
    pub rect: Rect,
    /// The size of the font in pixels.
    pub size: i32,
    pub bold: bool,
    pub color: Rgba,
}

/// Copies the frozen screen into `area` of the frame: as it is inside the selection and dimmed
/// outside it.
pub fn background(frame: &mut Frame, shot: &Image, selection: Option<Rect>, area: Rect) {
    debug_assert_eq!((frame.width, frame.height), (shot.width, shot.height));
    let stride = shot.stride();
    let bright = selection.map(|s| s.intersect(area)).filter(|s| !s.is_empty());
    for y in area.top..area.bottom {
        let row = y as usize * stride;
        let span = row + area.left as usize * 4..row + area.right as usize * 4;
        let source = &shot.pixels[span.clone()];
        let target = &mut frame.pixels[span];
        match bright.filter(|s| (s.top..s.bottom).contains(&y)) {
            Some(s) => {
                let from = (s.left - area.left) as usize * 4;
                let to = (s.right - area.left) as usize * 4;
                dim(&source[..from], &mut target[..from]);
                target[from..to].copy_from_slice(&source[from..to]);
                dim(&source[to..], &mut target[to..]);
            }
            None => dim(source, target),
        }
    }
}

fn dim(source: &[u8], target: &mut [u8]) {
    for (t, s) in target.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
        t[0] = ((s[0] as u16 * DIM) >> 8) as u8;
        t[1] = ((s[1] as u16 * DIM) >> 8) as u8;
        t[2] = ((s[2] as u16 * DIM) >> 8) as u8;
        t[3] = 255;
    }
}

/// Draws annotations into a layer the size of the selection, whose top left corner is at
/// `origin` on the screen.
pub fn annotations<'a>(layer: &mut Pixmap, items: impl IntoIterator<Item = &'a Annotation>, origin: Point) {
    let transform = Transform::from_translate(-origin.x as f32, -origin.y as f32);
    for item in items {
        annotation(layer, item, transform);
    }
}

fn annotation(pixmap: &mut Pixmap, item: &Annotation, transform: Transform) {
    let paint = solid(item.color);
    let stroke = Stroke { width: item.width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
    match &item.shape {
        Shape::Stroke(points) => {
            if let Some(path) = smooth(points) {
                pixmap.stroke_path(&path, &paint, &stroke, transform, None);
            } else if let Some(dot) = PathBuilder::from_circle(points[0].x, points[0].y, item.width / 2.0) {
                pixmap.fill_path(&dot, &paint, FillRule::Winding, transform, None);
            }
        }
        Shape::Line(from, to) => {
            let mut path = PathBuilder::new();
            path.move_to(from.x, from.y);
            path.line_to(to.x, to.y);
            if let Some(path) = path.finish() {
                pixmap.stroke_path(&path, &paint, &stroke, transform, None);
            }
        }
        Shape::Arrow(from, to) => arrow(pixmap, *from, *to, item, transform),
        Shape::Rectangle(a, b) => {
            if let Some(rect) = rect_between(*a, *b) {
                pixmap.stroke_path(&PathBuilder::from_rect(rect), &paint, &stroke, transform, None);
            }
        }
        Shape::FilledRectangle(a, b) => {
            if let Some(rect) = rect_between(*a, *b) {
                pixmap.fill_rect(rect, &paint, transform, None);
            }
        }
        Shape::Ellipse { center, radii, angle } => {
            let oval = tiny_skia::Rect::from_xywh(-radii.0, -radii.1, 2.0 * radii.0, 2.0 * radii.1);
            if let Some(path) = oval.and_then(PathBuilder::from_oval) {
                let placed = transform
                    .pre_concat(Transform::from_translate(center.x, center.y))
                    .pre_concat(Transform::from_rotate(angle.to_degrees()));
                pixmap.stroke_path(&path, &paint, &stroke, placed, None);
            }
        }
        Shape::Text(label) => text_label(pixmap, label, item.color, transform),
    }
}

/// A label: its letters in their colour over a soft shadow that falls a little below them,
/// which keeps them readable on a light or busy background. The shadow is dark, and light only
/// under dark letters, which a dark one wouldn't help.
fn text_label(pixmap: &mut Pixmap, label: &Label, color: Rgba, transform: Transform) {
    let at = Point::new(label.at.x + transform.tx.round() as i32, label.at.y + transform.ty.round() as i32);
    let coverage = &label.coverage;
    let [r, g, b, _] = color.map(|c| c as f32 / 255.0);
    let shade = if 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.2 { [255, 255, 255, 110] } else { [0, 0, 0, 130] };
    let (spread, fall) = shadow(label.size);
    let soft = blurred(coverage, spread as usize);
    cover(pixmap, Point::new(at.x, at.y + fall), coverage.width, &soft, shade);
    cover(pixmap, at, coverage.width, &coverage.values, color);
}

/// The coverage blurred by `by` pixels: two passes of an average over a box, along the rows and
/// then along the columns, which come close to a soft round blur.
fn blurred(coverage: &Coverage, by: usize) -> Vec<u8> {
    let (width, height) = (coverage.width as usize, coverage.height as usize);
    let mut values = coverage.values.clone();
    for _ in 0..2 {
        values = average(&values, width, height, by, true);
        values = average(&values, width, height, by, false);
    }
    values
}

/// Each value as the average of its neighbours within `by` along the rows or the columns.
fn average(values: &[u8], width: usize, height: usize, by: usize, rows: bool) -> Vec<u8> {
    let (lines, length) = if rows { (height, width) } else { (width, height) };
    let at = |line: usize, i: usize| if rows { line * width + i } else { i * width + line };
    let span = (2 * by + 1) as u32;
    let mut out = vec![0u8; values.len()];
    for line in 0..lines {
        let mut sum: u32 = (0..=by.min(length - 1)).map(|i| values[at(line, i)] as u32).sum();
        for i in 0..length {
            out[at(line, i)] = (sum / span) as u8;
            if i + by + 1 < length {
                sum += values[at(line, i + by + 1)] as u32;
            }
            if i >= by {
                sum -= values[at(line, i - by)] as u32;
            }
        }
    }
    out
}

/// Paints `color` over the layer where `values`, a coverage `width` pixels wide with its top
/// left corner at `at`, covers it.
fn cover(pixmap: &mut Pixmap, at: Point, width: u32, values: &[u8], color: Rgba) {
    let (layer_width, layer_height) = (pixmap.width() as i32, pixmap.height() as i32);
    let data = pixmap.data_mut();
    for (i, &value) in values.iter().enumerate() {
        if value == 0 {
            continue;
        }
        let (x, y) = (at.x + (i as u32 % width) as i32, at.y + (i as u32 / width) as i32);
        if x < 0 || y < 0 || x >= layer_width || y >= layer_height {
            continue;
        }
        let alpha = value as u32 * color[3] as u32 / 255;
        // The layer keeps its colours multiplied by their opacity, with red and blue swapped.
        let source = [color[2], color[1], color[0]].map(|c| c as u32 * alpha / 255);
        let pixel = &mut data[(y * layer_width + x) as usize * 4..][..4];
        let keep = 255 - alpha;
        for c in 0..3 {
            pixel[c] = (source[c] + (pixel[c] as u32 * keep + 127) / 255) as u8;
        }
        pixel[3] = (alpha + (pixel[3] as u32 * keep + 127) / 255) as u8;
    }
}

/// Lays one layer of annotations over another of the same size.
pub fn lay_over(layer: &mut Pixmap, top: &Pixmap) {
    layer.draw_pixmap(0, 0, top.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

/// Shows the annotation picked: white handles ringed in the accent colour at the ends of a line
/// or an arrow and the corners of a rectangle, or a thin dashed frame around an annotation that
/// only moves.
pub fn picked(frame: &mut Frame, item: &Annotation, scale: f32) {
    let Some(mut pixmap) = frame.pixmap() else { return };
    let handles = item.handles();
    if handles.is_empty() {
        let line = scale.round().max(1.0);
        let bounds = item.bounds();
        let half = line / 2.0;
        let edges = tiny_skia::Rect::from_ltrb(
            bounds.left as f32 + half,
            bounds.top as f32 + half,
            bounds.right as f32 - half,
            bounds.bottom as f32 - half,
        );
        let dash = StrokeDash::new(vec![4.0 * line, 3.0 * line], 0.0);
        if let Some(rect) = edges {
            let stroke = Stroke { width: line, dash, ..Stroke::default() };
            pixmap.stroke_path(&PathBuilder::from_rect(rect), &solid(ACCENT), &stroke, Transform::identity(), None);
        }
        return;
    }
    let radius = PICKED_RADIUS * scale;
    let ring = Stroke { width: (1.5 * scale).max(1.0), ..Stroke::default() };
    for handle in handles {
        if let Some(circle) = PathBuilder::from_circle(handle.x, handle.y, radius) {
            pixmap.fill_path(&circle, &solid(WHITE), FillRule::Winding, Transform::identity(), None);
            pixmap.stroke_path(&circle, &solid(ACCENT), &ring, Transform::identity(), None);
        }
    }
}

/// The part of the frame that shows the annotation picked covers.
pub fn picked_bounds(item: &Annotation, scale: f32) -> Rect {
    item.bounds().inflate((PICKED_RADIUS * scale + 1.5 * scale).ceil() as i32 + 1)
}

/// The caret of the text being written: a thin bar in its colour.
pub fn caret(frame: &mut Frame, rect: Rect, color: Rgba) {
    let Some(mut pixmap) = frame.pixmap() else { return };
    if let Some(bar) = tiny_skia::Rect::from_ltrb(rect.left as f32, rect.top as f32, rect.right as f32, rect.bottom as f32) {
        pixmap.fill_rect(bar, &solid([color[0], color[1], color[2], 255]), Transform::identity(), None);
    }
}

/// A smooth line through the points of a stroke: the middles of its segments are joined by
/// curves bent towards the points themselves. Nothing for a single point.
fn smooth(points: &[PointF]) -> Option<Path> {
    let (first, last) = (points.first()?, points.last()?);
    if points.len() < 2 || points.iter().all(|p| p == first) {
        return None;
    }
    let mut path = PathBuilder::new();
    path.move_to(first.x, first.y);
    for pair in points[1..].windows(2) {
        let (p, next) = (pair[0], pair[1]);
        path.quad_to(p.x, p.y, (p.x + next.x) / 2.0, (p.y + next.y) / 2.0);
    }
    path.line_to(last.x, last.y);
    path.finish()
}

fn arrow(pixmap: &mut Pixmap, from: PointF, to: PointF, item: &Annotation, transform: Transform) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = dx.hypot(dy);
    if length < 0.5 {
        return;
    }
    let (ux, uy) = (dx / length, dy / length);
    let head = arrow_head(item.width).min(length);
    let half = head * 0.55;
    let base = PointF::new(to.x - ux * head, to.y - uy * head);
    let paint = solid(item.color);

    // The shaft ends inside the head, so that its round end doesn't show.
    let shaft = PointF::new(to.x - ux * head * 0.6, to.y - uy * head * 0.6);
    let mut line = PathBuilder::new();
    line.move_to(from.x, from.y);
    line.line_to(shaft.x, shaft.y);
    let stroke = Stroke { width: item.width, line_cap: LineCap::Round, ..Stroke::default() };
    if let Some(line) = line.finish() {
        pixmap.stroke_path(&line, &paint, &stroke, transform, None);
    }

    let mut tip = PathBuilder::new();
    tip.move_to(to.x, to.y);
    tip.line_to(base.x - uy * half, base.y + ux * half);
    tip.line_to(base.x + uy * half, base.y - ux * half);
    tip.close();
    if let Some(tip) = tip.finish() {
        pixmap.fill_path(&tip, &paint, FillRule::Winding, transform, None);
        // A thin outline in the same colour rounds the corners of the head a little.
        let soft = Stroke { width: (item.width * 0.4).max(1.0), line_join: LineJoin::Round, ..Stroke::default() };
        pixmap.stroke_path(&tip, &paint, &soft, transform, None);
    }
}

fn rect_between(a: PointF, b: PointF) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_ltrb(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

/// Lays a layer with transparency over pixels in `area`: the pixels of the frame or of a
/// picture `width` pixels wide. The layer's top left corner is at `at`.
pub fn blend(pixels: &mut [u8], width: u32, layer: &Pixmap, at: Point, area: Rect) {
    let layer_rect = Rect::new(at.x, at.y, at.x + layer.width() as i32, at.y + layer.height() as i32);
    let overlap = area.intersect(layer_rect);
    if overlap.is_empty() {
        return;
    }
    let (data, layer_width) = (layer.data(), layer.width() as usize);
    for y in overlap.top..overlap.bottom {
        let row = (y - at.y) as usize * layer_width;
        let source = &data[(row + (overlap.left - at.x) as usize) * 4..(row + (overlap.right - at.x) as usize) * 4];
        let start = (y as usize * width as usize + overlap.left as usize) * 4;
        let target = &mut pixels[start..start + source.len()];
        for (t, s) in target.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
            // The layer's colours are already multiplied by their opacity.
            match s[3] {
                0 => {}
                255 => t[..3].copy_from_slice(&s[..3]),
                alpha => {
                    let keep = 255 - alpha as u32;
                    for c in 0..3 {
                        t[c] = (s[c] as u32 + (t[c] as u32 * keep + 127) / 255) as u8;
                    }
                }
            }
        }
    }
}

/// A thin frame just outside the selection, so that it covers none of the selected pixels, and
/// round handles at the corners and in the middle of long enough sides.
pub fn selection_frame(frame: &mut Frame, selection: Rect, scale: f32) {
    let Some(mut pixmap) = frame.pixmap() else { return };
    let line = scale.round().max(1.0);
    let half = line / 2.0;
    let left = selection.left as f32 - half;
    let top = selection.top as f32 - half;
    let right = selection.right as f32 + half;
    let bottom = selection.bottom as f32 + half;
    if let Some(rect) = tiny_skia::Rect::from_ltrb(left, top, right, bottom) {
        let stroke = Stroke { width: line, ..Stroke::default() };
        let path = PathBuilder::from_rect(rect);
        pixmap.stroke_path(&path, &solid(FRAME), &stroke, Transform::identity(), None);
    }

    let radius = HANDLE_RADIUS * scale;
    let mut handles = vec![(left, top), (right, top), (right, bottom), (left, bottom)];
    let (middle_x, middle_y) = ((left + right) / 2.0, (top + bottom) / 2.0);
    if right - left > radius * 8.0 {
        handles.extend([(middle_x, top), (middle_x, bottom)]);
    }
    if bottom - top > radius * 8.0 {
        handles.extend([(left, middle_y), (right, middle_y)]);
    }
    let ring = Stroke { width: scale.max(1.0), ..Stroke::default() };
    for (x, y) in handles {
        if let Some(circle) = PathBuilder::from_circle(x, y, radius) {
            pixmap.fill_path(&circle, &solid(WHITE), FillRule::Winding, Transform::identity(), None);
            pixmap.stroke_path(&circle, &solid(HANDLE_RING), &ring, Transform::identity(), None);
        }
    }
}

/// A frame in the accent colour along the inside of the window that a click would select, so
/// that it shows even on a window that fills the screen.
pub fn window_frame(frame: &mut Frame, window: Rect, scale: f32) {
    let Some(mut pixmap) = frame.pixmap() else { return };
    let line = (2.0 * scale).round().max(2.0);
    let half = line / 2.0;
    let edges = tiny_skia::Rect::from_ltrb(
        window.left as f32 + half,
        window.top as f32 + half,
        window.right as f32 - half,
        window.bottom as f32 - half,
    );
    if let Some(rect) = edges {
        let stroke = Stroke { width: line, ..Stroke::default() };
        pixmap.stroke_path(&PathBuilder::from_rect(rect), &solid(ACCENT), &stroke, Transform::identity(), None);
    }
}

/// The part of the frame covered by the selection's frame and handles.
pub fn selection_bounds(selection: Rect, scale: f32) -> Rect {
    selection.inflate((HANDLE_RADIUS * scale).ceil() as i32 + 2)
}

/// The size of the font of labels and hints, in pixels.
pub fn label_font(scale: f32) -> i32 {
    px(LABEL_FONT, scale)
}

/// The size of a pill around text of the given size.
pub fn pill_size(text: (i32, i32), scale: f32) -> (i32, i32) {
    (text.0 + 2 * px(PILL_PADDING_X, scale), text.1 + 2 * px(PILL_PADDING_Y, scale))
}

/// Where the label with the selection's size goes: above the selection at its left end, or at
/// its right end, or below it, wherever there is room on the monitor clear of the panels; inside
/// the selection when there is no such place.
pub fn label_place(selection: Rect, size: (i32, i32), monitor: Rect, scale: f32, panels: &[Rect]) -> Rect {
    let gap = px(LABEL_GAP, scale);
    let (above, below) = (selection.top - gap - size.1, selection.bottom + gap);
    let (left, right) = (selection.left, selection.right - size.0);
    let at = |left: i32, top: i32| {
        let left = left.min(monitor.right - size.0).max(monitor.left);
        Rect::new(left, top, left + size.0, top + size.1)
    };
    let clear = |place: &Rect| {
        place.top >= monitor.top
            && place.bottom <= monitor.bottom
            && panels.iter().all(|panel| panel.inflate(gap).intersect(*place).is_empty())
    };
    [at(left, above), at(right, above), at(left, below), at(right, below)]
        .into_iter()
        .find(clear)
        .unwrap_or_else(|| at(left, selection.top + gap))
}

/// A dark pill under a label or a hint; its text is drawn over it by the system.
pub fn pill(frame: &mut Frame, rect: Rect) {
    let Some(mut pixmap) = frame.pixmap() else { return };
    let (width, height) = (rect.width() as f32, rect.height() as f32);
    if let Some(shape) = rounded_rect(rect.left as f32, rect.top as f32, width, height, height / 2.0) {
        pixmap.fill_path(&shape, &solid(PILL), FillRule::Winding, Transform::identity(), None);
    }
}

/// How the toolbar's buttons look at the moment.
pub struct ToolbarLook<'a> {
    pub tool: Option<Tool>,
    pub hovered: Option<Button>,
    pub color: Rgba,
    /// The line width at 100%, shown by the size of the colour's dot.
    pub width: f32,
    pub can_undo: bool,
    /// The query button whose screenshot is being sent; the other buttons wait meanwhile.
    pub busy: Option<usize>,
    /// The icons of the query buttons.
    pub queries: &'a [Icon],
}

/// Draws the toolbar and returns the letters that some buttons show instead of icons.
pub fn toolbar(frame: &mut Frame, bar: &Toolbar, look: &ToolbarLook) -> Vec<Text> {
    let Some(mut pixmap) = frame.pixmap() else { return Vec::new() };
    let scale = bar.scale;
    panel(&mut pixmap, bar.rect, scale);
    for s in &bar.separators {
        // A thin line across the gap between two groups: upright in a row, level in a column.
        let (x, y, w, h) = (s.left as f32, s.top as f32, s.width() as f32, s.height() as f32);
        let line = if w < h {
            tiny_skia::Rect::from_xywh(x + w / 2.0 - 0.5, y + h * 0.22, 1.0, h * 0.56)
        } else {
            tiny_skia::Rect::from_xywh(x + w * 0.22, y + h / 2.0 - 0.5, w * 0.56, 1.0)
        };
        if let Some(line) = line {
            pixmap.fill_rect(line, &solid(SEPARATOR), Transform::identity(), None);
        }
    }

    let mut letters = Vec::new();
    for &(button, rect) in &bar.buttons {
        let active = match button {
            Button::Tool(tool) => look.tool == Some(tool),
            Button::Query(i) => look.busy == Some(i),
            _ => false,
        };
        let enabled = look.busy.is_none() && (button != Button::Undo || look.can_undo);
        let inset = (2.0 * scale).round();
        let back = rounded_rect(
            rect.left as f32 + inset,
            rect.top as f32 + inset,
            rect.width() as f32 - 2.0 * inset,
            rect.height() as f32 - 2.0 * inset,
            BUTTON_RADIUS * scale,
        );
        if let Some(back) = &back {
            if active {
                pixmap.fill_path(back, &solid(ACCENT), FillRule::Winding, Transform::identity(), None);
            } else if enabled && look.hovered == Some(button) {
                pixmap.fill_path(back, &solid(HOVER), FillRule::Winding, Transform::identity(), None);
            }
        }
        let ink = if active { WHITE } else if enabled { INK } else { INK_OFF };
        let glyph = match button {
            Button::Tool(Tool::Pencil) => icons::PENCIL,
            Button::Tool(Tool::Brush) => icons::BRUSH,
            Button::Tool(Tool::Line) => icons::LINE_TOOL,
            Button::Tool(Tool::Arrow) => icons::ARROW,
            Button::Tool(Tool::Rectangle) => icons::RECTANGLE,
            Button::Tool(Tool::FilledRectangle) => icons::FILLED_RECTANGLE,
            Button::Tool(Tool::Text) => icons::TEXT,
            Button::Tool(Tool::MagicPencil) => icons::MAGIC_PENCIL,
            Button::Color => {
                color_dot(&mut pixmap, rect, look.color, look.width, scale);
                continue;
            }
            Button::Undo => icons::UNDO,
            Button::Copy => icons::COPY,
            Button::CopyText => icons::TEXT_COPY,
            Button::Save => icons::SAVE,
            Button::Close => icons::CLOSE,
            Button::Query(i) => {
                let shown = match look.queries.get(i).copied().unwrap_or(Icon::Images) {
                    Icon::Google => Some("G"),
                    Icon::Translate => Some("AЯ"),
                    Icon::Images => {
                        icon(&mut pixmap, icons::IMAGES, rect, ink, scale);
                        None
                    }
                    Icon::Question => {
                        icon(&mut pixmap, icons::QUESTION, rect, ink, scale);
                        None
                    }
                };
                if let Some(shown) = shown {
                    let size = if shown.chars().count() > 1 { 13.0 } else { 17.0 };
                    letters.push(Text { text: shown.into(), rect, size: px(size, scale), bold: true, color: ink });
                }
                continue;
            }
        };
        icon(&mut pixmap, glyph, rect, ink, scale);
    }
    letters
}

/// The panel under the toolbar and the palette: dark, rounded, with a soft shadow.
fn panel(pixmap: &mut PixmapMut, rect: Rect, scale: f32) {
    let (x, y, w, h) = (rect.left as f32, rect.top as f32, rect.width() as f32, rect.height() as f32);
    let radius = PANEL_RADIUS * scale;
    for (spread, alpha) in [(4.0, 10), (2.5, 16), (1.0, 26)] {
        let s = spread * scale;
        if let Some(shadow) = rounded_rect(x - s, y - s + scale, w + 2.0 * s, h + 2.0 * s, radius + s) {
            pixmap.fill_path(&shadow, &solid([0, 0, 0, alpha]), FillRule::Winding, Transform::identity(), None);
        }
    }
    if let Some(shape) = rounded_rect(x, y, w, h, radius) {
        pixmap.fill_path(&shape, &solid(PANEL), FillRule::Winding, Transform::identity(), None);
        let edge = Stroke { width: 1.0, ..Stroke::default() };
        pixmap.stroke_path(&shape, &solid(PANEL_EDGE), &edge, Transform::identity(), None);
    }
}

/// How far the toolbar's shadow reaches beyond it.
pub fn shadow_reach(scale: f32) -> i32 {
    (5.0 * scale).ceil() as i32 + 1
}

/// The colour button: a dot of the current colour whose size follows the line width.
fn color_dot(pixmap: &mut PixmapMut, rect: Rect, color: Rgba, width: f32, scale: f32) {
    let (cx, cy) = ((rect.left + rect.right) as f32 / 2.0, (rect.top + rect.bottom) as f32 / 2.0);
    let radius = (3.0 + width * 0.5).clamp(4.0, 9.0) * scale;
    if let Some(dot) = PathBuilder::from_circle(cx, cy, radius) {
        pixmap.fill_path(&dot, &solid([color[0], color[1], color[2], 255]), FillRule::Winding, Transform::identity(), None);
        let ring = Stroke { width: 1.5 * scale, ..Stroke::default() };
        pixmap.stroke_path(&dot, &solid([255, 255, 255, 200]), &ring, Transform::identity(), None);
    }
}

/// An icon centred in `rect`.
fn icon(pixmap: &mut PixmapMut, parts: &[Part], rect: Rect, color: Rgba, scale: f32) {
    let size = ICON * scale;
    let left = rect.left as f32 + (rect.width() as f32 - size) / 2.0;
    let top = rect.top as f32 + (rect.height() as f32 - size) / 2.0;
    let place = Transform::from_row(size / icons::GRID, 0.0, 0.0, size / icons::GRID, left, top);
    let paint = solid(color);
    let stroke = Stroke { width: icons::LINE, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
    for part in parts {
        match part {
            Part::Line(d) => {
                if let Some(path) = icons::path(d) {
                    pixmap.stroke_path(&path, &paint, &stroke, place, None);
                }
            }
            Part::Fill(d) => {
                if let Some(path) = icons::path(d) {
                    pixmap.fill_path(&path, &paint, FillRule::Winding, place, None);
                }
            }
            Part::Dot(x, y, r) => {
                if let Some(dot) = PathBuilder::from_circle(*x, *y, *r) {
                    pixmap.fill_path(&dot, &paint, FillRule::Winding, place, None);
                }
            }
        }
    }
}

/// The colours that open from the colour button, with the current one ringed.
pub fn palette(frame: &mut Frame, palette: &Palette, colors: &[Rgba], current: usize, scale: f32) {
    let Some(mut pixmap) = frame.pixmap() else { return };
    panel(&mut pixmap, palette.rect, scale);
    for (i, (rect, color)) in palette.swatches.iter().zip(colors).enumerate() {
        let (cx, cy) = ((rect.left + rect.right) as f32 / 2.0, (rect.top + rect.bottom) as f32 / 2.0);
        let radius = 8.0 * scale;
        if let Some(dot) = PathBuilder::from_circle(cx, cy, radius) {
            pixmap.fill_path(&dot, &solid([color[0], color[1], color[2], 255]), FillRule::Winding, Transform::identity(), None);
            let edge = Stroke { width: 1.0, ..Stroke::default() };
            pixmap.stroke_path(&dot, &solid([255, 255, 255, 60]), &edge, Transform::identity(), None);
        }
        if i == current
            && let Some(ring) = PathBuilder::from_circle(cx, cy, radius + 3.0 * scale)
        {
            let stroke = Stroke { width: 2.0 * scale, ..Stroke::default() };
            pixmap.stroke_path(&ring, &solid(WHITE), &stroke, Transform::identity(), None);
        }
    }
}

/// A ring at the mouse showing the line width while the wheel changes it.
pub fn width_ring(frame: &mut Frame, center: Point, diameter: f32) {
    let Some(mut pixmap) = frame.pixmap() else { return };
    let radius = (diameter / 2.0).max(1.5);
    if let Some(ring) = PathBuilder::from_circle(center.x as f32, center.y as f32, radius) {
        let dark = Stroke { width: 3.0, ..Stroke::default() };
        pixmap.stroke_path(&ring, &solid([0, 0, 0, 120]), &dark, Transform::identity(), None);
        let light = Stroke { width: 1.5, ..Stroke::default() };
        pixmap.stroke_path(&ring, &solid(WHITE), &light, Transform::identity(), None);
    }
}

/// The part of the frame the width ring covers.
pub fn ring_bounds(center: Point, diameter: f32) -> Rect {
    let reach = (diameter / 2.0).ceil() as i32 + 4;
    Rect::new(center.x - reach, center.y - reach, center.x + reach + 1, center.y + reach + 1)
}

/// How far from an edge of the selection the mouse still takes it, in pixels.
pub fn edge_reach(scale: f32) -> i32 {
    px(EDGE_REACH, scale)
}

/// The program's icon: white corners of a frame on a blue rounded square.
pub fn app_icon(size: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(size, size)?;
    let unit = size as f32 / 16.0;
    let tile = rounded_rect(0.5 * unit, 0.5 * unit, 15.0 * unit, 15.0 * unit, 3.5 * unit)?;
    pixmap.fill_path(&tile, &solid(ACCENT), FillRule::Winding, Transform::identity(), None);

    let corners = [
        [(4.0, 7.0), (4.0, 4.0), (7.0, 4.0)],
        [(9.0, 4.0), (12.0, 4.0), (12.0, 7.0)],
        [(12.0, 9.0), (12.0, 12.0), (9.0, 12.0)],
        [(7.0, 12.0), (4.0, 12.0), (4.0, 9.0)],
    ];
    let mut path = PathBuilder::new();
    for [start, corner, end] in corners {
        path.move_to(start.0 * unit, start.1 * unit);
        path.line_to(corner.0 * unit, corner.1 * unit);
        path.line_to(end.0 * unit, end.1 * unit);
    }
    let stroke = Stroke { width: 1.5 * unit, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
    pixmap.stroke_path(&path.finish()?, &solid(WHITE), &stroke, Transform::identity(), None);
    Some(pixmap)
}

/// A paint of one colour, with red and blue swapped for the pixel order used here.
fn solid(color: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[2], color[1], color[0], color[3]);
    paint
}

/// A rectangle with corners rounded to the radius `r`.
fn rounded_rect(x: f32, y: f32, width: f32, height: f32, r: f32) -> Option<Path> {
    // How far the control points of a cubic curve that draws a quarter circle lie from its
    // ends, as a part of the radius.
    const K: f32 = 0.552_285;
    let r = r.min(width / 2.0).min(height / 2.0);
    let (right, bottom) = (x + width, y + height);
    let mut path = PathBuilder::new();
    path.move_to(x + r, y);
    path.line_to(right - r, y);
    path.cubic_to(right - r + r * K, y, right, y + r - r * K, right, y + r);
    path.line_to(right, bottom - r);
    path.cubic_to(right, bottom - r + r * K, right - r + r * K, bottom, right - r, bottom);
    path.line_to(x + r, bottom);
    path.cubic_to(x + r - r * K, bottom, x, bottom - r + r * K, x, bottom - r);
    path.line_to(x, y + r);
    path.cubic_to(x, y + r - r * K, x + r - r * K, y, x + r, y);
    path.close();
    path.finish()
}

/// A size at 100% turned into whole pixels at the given scale.
fn px(size: f32, scale: f32) -> i32 {
    (size * scale).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::Draft;

    /// Draws the overlay over a made-up screen into the PNG file named by `SCREENSHOT_PREVIEW`,
    /// to look at the toolbar and the annotations without running the program:
    /// `SCREENSHOT_PREVIEW=preview.png cargo test -p ainitshot preview -- --ignored`, with
    /// `SCREENSHOT_PANELS='tools = { side = "left", align = "top" }'` for the panels elsewhere.
    #[test]
    #[ignore]
    fn preview() {
        let (width, height) = (900, 520);
        let mut pixels = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let shade = 200 + ((x / 40 + y / 40) % 2 * 30) as u8;
                pixels.extend_from_slice(&[shade, shade - 20, shade - 60, 255]);
            }
        }
        let screen = Image { width: width as u32, height: height as u32, pixels };
        let mut target = vec![0; width * height * 4];
        let mut frame = Frame { pixels: &mut target, width: width as u32, height: height as u32 };
        let selection = Rect::new(120, 60, 780, 330);
        background(&mut frame, &screen, Some(selection), screen.bounds());

        let red = [229, 57, 53, 255];
        let mut layer = Pixmap::new(selection.width() as u32, selection.height() as u32).unwrap();
        let mut drawn = Vec::new();
        for (tool, points) in [
            (Tool::Arrow, vec![(160.0, 290.0), (330.0, 150.0)]),
            (Tool::Rectangle, vec![(380.0, 100.0), (520.0, 190.0)]),
            (Tool::FilledRectangle, vec![(560.0, 230.0), (740.0, 290.0)]),
            (Tool::Brush, vec![(380.0, 260.0), (430.0, 262.0), (520.0, 258.0)]),
            (Tool::Pencil, vec![(560.0, 120.0), (600.0, 90.0), (650.0, 140.0), (700.0, 100.0)]),
        ] {
            let mut draft = Draft::start(tool, PointF::new(points[0].0, points[0].1), red, 4.0).unwrap();
            for &(x, y) in &points[1..] {
                draft.extend(PointF::new(x, y), false);
            }
            drawn.extend(draft.finish(false));
        }
        annotations(&mut layer, &drawn, selection.top_left());
        blend(frame.pixels, frame.width, &layer, selection.top_left(), selection);
        // The arrow picked shows its handles; the pencil's line, which only moves, its frame.
        picked(&mut frame, &drawn[0], 1.0);
        picked(&mut frame, &drawn[4], 1.0);
        selection_frame(&mut frame, selection, 1.0);

        // The panels stand where `SCREENSHOT_PANELS` puts them, as the [panels] of the settings
        // file do, or where they stand by default.
        let panels: crate::settings::Panels = std::env::var("SCREENSHOT_PANELS")
            .map(|text| toml::from_str(&text).expect("SCREENSHOT_PANELS reads as [panels]"))
            .unwrap_or_default();
        let (bar, column) = Toolbar::panels(selection, &panels, &[vec![0, 1], vec![2]], |_| (screen.bounds(), 1.0));
        let column = column.unwrap();
        let queries = [Icon::Google, Icon::Translate, Icon::Images];
        let look = ToolbarLook {
            tool: Some(Tool::Arrow),
            hovered: Some(Button::Query(2)),
            color: red,
            width: 4.0,
            can_undo: true,
            busy: None,
            queries: &queries,
        };
        toolbar(&mut frame, &bar, &look);
        toolbar(&mut frame, &column, &look);
        if let Some(colors) = Palette::layout(&bar, 8) {
            palette(&mut frame, &colors, &[red, [251, 140, 0, 255], [253, 216, 53, 255], [67, 160, 71, 255], [30, 136, 229, 255], [142, 36, 170, 255], [33, 33, 33, 255], [255, 255, 255, 255]], 0, 1.0);
        }

        let path = std::env::var("SCREENSHOT_PREVIEW").expect("SCREENSHOT_PREVIEW names the file to write");
        let picture = Image { width: width as u32, height: height as u32, pixels: target };
        std::fs::write(path, picture.to_png().unwrap()).unwrap();
    }

    #[test]
    fn a_translucent_layer_is_mixed_with_the_pixels_under_it() {
        let mut layer = Pixmap::new(2, 1).unwrap();
        // Half-opaque white, already multiplied by its opacity, and a fully opaque pixel.
        layer.data_mut().copy_from_slice(&[128, 128, 128, 128, 10, 20, 30, 255]);
        let mut pixels = vec![0, 0, 0, 255, 200, 200, 200, 255, 99, 99, 99, 255];
        blend(&mut pixels, 3, &layer, Point::new(1, 0), Rect::new(0, 0, 3, 1));
        assert_eq!(&pixels[..4], &[0, 0, 0, 255]);
        assert_eq!(&pixels[4..8], &[228, 228, 228, 255]);
        assert_eq!(&pixels[8..], &[10, 20, 30, 255]);
    }
}
