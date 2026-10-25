//! Annotations on the selection: what each tool makes of a drag, and the history that Ctrl+Z
//! goes back through. Annotations keep their place on the screen, so they stay where they
//! were drawn when the selection is moved.

use crate::color::Rgba;
use crate::geometry::{Point, PointF, Rect};
use crate::magic;
use crate::text::tr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pencil,
    Brush,
    Line,
    Arrow,
    Rectangle,
    FilledRectangle,
    /// Writes text where the selection is clicked.
    Text,
    MagicPencil,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Pencil,
        Tool::Brush,
        Tool::Line,
        Tool::Arrow,
        Tool::Rectangle,
        Tool::FilledRectangle,
        Tool::Text,
        Tool::MagicPencil,
    ];

    /// The tool's name, as its hint and the settings say it.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Pencil => tr("Pencil"),
            Tool::Brush => tr("Brush"),
            Tool::Line => tr("Line"),
            Tool::Arrow => tr("Arrow"),
            Tool::Rectangle => tr("Rectangle"),
            Tool::FilledRectangle => tr("Filled rectangle"),
            Tool::Text => tr("Text"),
            Tool::MagicPencil => tr("Magic pencil"),
        }
    }

    /// The key that picks the tool while the screen is frozen. Query buttons can't have it.
    pub fn key(self) -> char {
        match self {
            Tool::Pencil => 'P',
            Tool::Brush => 'B',
            Tool::Line => 'L',
            Tool::Arrow => 'A',
            Tool::Rectangle => 'R',
            Tool::FilledRectangle => 'F',
            // N, the key that has Т on the Russian layout.
            Tool::Text => 'N',
            Tool::MagicPencil => 'M',
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A line along the mouse: the pencil, the brush, and the magic pencil's loop when no
    /// neat shape fits it.
    Stroke(Vec<PointF>),
    Line(PointF, PointF),
    /// An arrow from the first point to the second, with its head at the second.
    Arrow(PointF, PointF),
    /// A frame between two opposite corners.
    Rectangle(PointF, PointF),
    FilledRectangle(PointF, PointF),
    /// An ellipse turned by `angle` radians.
    Ellipse { center: PointF, radii: (f32, f32), angle: f32 },
    Text(Label),
}

/// Text written on the selection.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// The text as typed, its lines split by `\n`.
    pub text: String,
    /// The size of its letters in pixels of the screen.
    pub size: f32,
    /// Where the top left corner of the picture of its letters is on the screen.
    pub at: Point,
    pub coverage: Coverage,
}

/// How much each pixel of a picture is covered, from 0 to 255, rows from the top: the letters
/// of a label as Windows drew them, which any colour can be drawn with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Coverage {
    pub width: u32,
    pub height: u32,
    pub values: Vec<u8>,
}

/// The size of a label's letters for a line width: the wheel makes text bigger as it makes lines
/// thicker.
pub fn text_size(width: f32) -> f32 {
    12.0 + 2.0 * width
}

/// How far the soft shadow under a label's letters spreads, and how far down it falls, in pixels
/// of the screen: the shadow keeps the letters readable on a busy or light background.
pub fn shadow(size: f32) -> (i32, i32) {
    ((size / 12.0).round().max(1.0) as i32, (size / 18.0).round().max(1.0) as i32)
}

/// The room left around a label's letters in their picture, for their shadow.
pub fn text_margin(size: f32) -> i32 {
    let (spread, fall) = shadow(size);
    2 * spread + fall + 1
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub shape: Shape,
    pub color: Rgba,
    /// The width of the line in pixels of the screen.
    pub width: f32,
}

/// The brush is a wide translucent line, like a highlighter: this many times wider than the
/// line width, with this opacity.
const BRUSH_WIDTH: f32 = 3.0;
const BRUSH_OPACITY: u8 = 110;

/// The width of the line a tool draws for the chosen line width.
pub fn drawn_width(tool: Option<Tool>, width: f32) -> f32 {
    if tool == Some(Tool::Brush) { width * BRUSH_WIDTH } else { width }
}

/// The length of an arrow's head for a line of the given width.
pub fn arrow_head(width: f32) -> f32 {
    (width * 3.5).max(12.0)
}

impl Annotation {
    /// The part of the screen the annotation covers.
    pub fn bounds(&self) -> Rect {
        let pad = self.width / 2.0 + 2.0;
        match &self.shape {
            Shape::Stroke(points) => Rect::bounding(points.iter().copied(), pad),
            Shape::Line(a, b) | Shape::Rectangle(a, b) | Shape::FilledRectangle(a, b) => {
                Rect::bounding([*a, *b], pad)
            }
            Shape::Arrow(a, b) => Rect::bounding([*a, *b], pad + arrow_head(self.width)),
            Shape::Ellipse { center, radii, .. } => {
                let r = radii.0.max(radii.1);
                let corners = [PointF::new(center.x - r, center.y - r), PointF::new(center.x + r, center.y + r)];
                Rect::bounding(corners, pad)
            }
            Shape::Text(label) => Rect::new(
                label.at.x,
                label.at.y,
                label.at.x + label.coverage.width as i32,
                label.at.y + label.coverage.height as i32,
            ),
        }
    }
}

/// The annotation being drawn.
pub struct Draft {
    tool: Tool,
    start: PointF,
    annotation: Annotation,
}

impl Draft {
    /// The mouse button goes down at `at` with the given tool, colour and line width. The text
    /// tool writes rather than draws: nothing.
    pub fn start(tool: Tool, at: PointF, color: Rgba, width: f32) -> Option<Draft> {
        let (color, width) = match tool {
            Tool::Brush => ([color[0], color[1], color[2], BRUSH_OPACITY], width * BRUSH_WIDTH),
            _ => (color, width),
        };
        let shape = match tool {
            Tool::Pencil | Tool::Brush | Tool::MagicPencil => Shape::Stroke(vec![at]),
            Tool::Line => Shape::Line(at, at),
            Tool::Arrow => Shape::Arrow(at, at),
            Tool::Rectangle => Shape::Rectangle(at, at),
            Tool::FilledRectangle => Shape::FilledRectangle(at, at),
            Tool::Text => return None,
        };
        Some(Draft { tool, start: at, annotation: Annotation { shape, color, width } })
    }

    pub fn annotation(&self) -> &Annotation {
        &self.annotation
    }

    /// The mouse moves to `to`. With `straight` (Shift held) lines keep to multiples of 45°
    /// and rectangles are square.
    pub fn extend(&mut self, to: PointF, straight: bool) {
        let start = self.start;
        match &mut self.annotation.shape {
            Shape::Stroke(points) => {
                if points.last().is_none_or(|last| last.distance(to) >= 1.0) {
                    points.push(to);
                }
            }
            Shape::Line(_, end) | Shape::Arrow(_, end) => {
                *end = if straight { snap_angle(start, to) } else { to };
            }
            Shape::Rectangle(_, end) | Shape::FilledRectangle(_, end) => {
                *end = if straight { square(start, to) } else { to };
            }
            Shape::Ellipse { .. } | Shape::Text(_) => {}
        }
    }

    /// The finished annotation, or nothing if it came out empty. The magic pencil turns its
    /// loop into the neat shape that fits it, unless `as_drawn` (Shift held) keeps it.
    pub fn finish(self, as_drawn: bool) -> Option<Annotation> {
        let Draft { tool, mut annotation, .. } = self;
        let fitted = match &annotation.shape {
            Shape::Line(a, b) | Shape::Arrow(a, b) if a.distance(*b) < 2.0 => return None,
            Shape::Rectangle(a, b) | Shape::FilledRectangle(a, b)
                if (a.x - b.x).abs() < 2.0 || (a.y - b.y).abs() < 2.0 =>
            {
                return None;
            }
            Shape::Stroke(points) if tool == Tool::MagicPencil && !as_drawn => magic::fit(points),
            _ => None,
        };
        if let Some(shape) = fitted {
            annotation.shape = shape;
        }
        Some(annotation)
    }
}

/// What of an annotation is under the mouse: one of its handles, which reshapes it, or the
/// annotation itself, which moves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Handle(usize),
    Body,
}

impl Annotation {
    /// The handles that reshape the annotation: the ends of a line or an arrow, the corners of a
    /// rectangle. Other annotations only move.
    pub fn handles(&self) -> Vec<PointF> {
        match &self.shape {
            Shape::Line(a, b) | Shape::Arrow(a, b) => vec![*a, *b],
            Shape::Rectangle(a, b) | Shape::FilledRectangle(a, b) => {
                vec![*a, PointF::new(b.x, a.y), *b, PointF::new(a.x, b.y)]
            }
            _ => Vec::new(),
        }
    }

    /// The handle within `reach` of the point, the closest one.
    pub fn handle_at(&self, p: PointF, reach: f32) -> Option<usize> {
        let handles = self.handles();
        let near = handles.iter().enumerate().filter(|(_, h)| h.distance(p) <= reach);
        near.min_by(|(_, a), (_, b)| a.distance(p).total_cmp(&b.distance(p))).map(|(i, _)| i)
    }

    /// Whether the point is on the annotation, within `reach` of what it draws: its line, the
    /// frame of a rectangle, the inside of a filled one or of a label.
    pub fn touches(&self, p: PointF, reach: f32) -> bool {
        let near = reach + self.width / 2.0;
        match &self.shape {
            Shape::Stroke(points) => match points.as_slice() {
                [only] => only.distance(p) <= near,
                points => points.windows(2).any(|pair| segment_distance(p, pair[0], pair[1]) <= near),
            },
            Shape::Line(a, b) | Shape::Arrow(a, b) => segment_distance(p, *a, *b) <= near,
            Shape::Rectangle(a, b) => {
                let corners = [*a, PointF::new(b.x, a.y), *b, PointF::new(a.x, b.y)];
                (0..4).any(|i| segment_distance(p, corners[i], corners[(i + 1) % 4]) <= near)
            }
            Shape::FilledRectangle(a, b) => {
                (a.x.min(b.x) - reach..=a.x.max(b.x) + reach).contains(&p.x)
                    && (a.y.min(b.y) - reach..=a.y.max(b.y) + reach).contains(&p.y)
            }
            Shape::Ellipse { center, radii, angle } => {
                // The point in the ellipse's own axes: on the outline the scaled radius is 1.
                let (dx, dy) = (p.x - center.x, p.y - center.y);
                let (sin, cos) = angle.sin_cos();
                let (x, y) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                let scaled = ((x / radii.0).powi(2) + (y / radii.1).powi(2)).sqrt();
                (scaled - 1.0).abs() * radii.0.min(radii.1) <= near
            }
            Shape::Text(_) => self.bounds().contains(Point::new(p.x.floor() as i32, p.y.floor() as i32)),
        }
    }

    /// The annotation moved by `dx` and `dy` pixels.
    pub fn moved(&self, dx: f32, dy: f32) -> Annotation {
        let by = |p: &PointF| PointF::new(p.x + dx, p.y + dy);
        let shape = match &self.shape {
            Shape::Stroke(points) => Shape::Stroke(points.iter().map(by).collect()),
            Shape::Line(a, b) => Shape::Line(by(a), by(b)),
            Shape::Arrow(a, b) => Shape::Arrow(by(a), by(b)),
            Shape::Rectangle(a, b) => Shape::Rectangle(by(a), by(b)),
            Shape::FilledRectangle(a, b) => Shape::FilledRectangle(by(a), by(b)),
            Shape::Ellipse { center, radii, angle } => Shape::Ellipse { center: by(center), radii: *radii, angle: *angle },
            Shape::Text(label) => Shape::Text(Label {
                at: Point::new(label.at.x + dx.round() as i32, label.at.y + dy.round() as i32),
                ..label.clone()
            }),
        };
        Annotation { shape, ..self.clone() }
    }

    /// The annotation with a handle dragged to `to`: an end of a line moves while the other stays,
    /// a corner of a rectangle moves while the opposite one stays. With `straight` (Shift held)
    /// lines keep to multiples of 45° and rectangles are square.
    pub fn reshaped(&self, handle: usize, to: PointF, straight: bool) -> Annotation {
        let shape = match &self.shape {
            Shape::Line(a, b) | Shape::Arrow(a, b) => {
                let (still, moving) = if handle == 0 { (*b, to) } else { (*a, to) };
                let moving = if straight { snap_angle(still, moving) } else { moving };
                let (a, b) = if handle == 0 { (moving, still) } else { (still, moving) };
                match self.shape {
                    Shape::Line(..) => Shape::Line(a, b),
                    _ => Shape::Arrow(a, b),
                }
            }
            Shape::Rectangle(..) | Shape::FilledRectangle(..) => {
                let corners = self.handles();
                let still = corners[(handle + 2) % 4];
                let moving = if straight { square(still, to) } else { to };
                match self.shape {
                    Shape::Rectangle(..) => Shape::Rectangle(still, moving),
                    _ => Shape::FilledRectangle(still, moving),
                }
            }
            shape => shape.clone(),
        };
        Annotation { shape, ..self.clone() }
    }
}

/// How far the point is from the segment between `a` and `b`.
fn segment_distance(p: PointF, a: PointF, b: PointF) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx * dx + dy * dy;
    if length == 0.0 {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / length).clamp(0.0, 1.0);
    p.distance(PointF::new(a.x + t * dx, a.y + t * dy))
}

/// The annotations drawn so far, and how they were before each change, for undo; and how they
/// were before each undo, for redo, until something new changes them.
#[derive(Default)]
pub struct History {
    done: Vec<Annotation>,
    past: Vec<Vec<Annotation>>,
    future: Vec<Vec<Annotation>>,
}

impl History {
    pub fn items(&self) -> &[Annotation] {
        &self.done
    }

    pub fn is_empty(&self) -> bool {
        self.done.is_empty()
    }

    pub fn push(&mut self, annotation: Annotation) {
        self.remember();
        self.done.push(annotation);
    }

    /// Remembers the annotations as they are, before a change that undo takes back whole: one
    /// drawn, moved, reshaped, rewritten or removed.
    pub fn remember(&mut self) {
        self.past.push(self.done.clone());
        self.future.clear();
    }

    /// Forgets the annotations remembered last if they are as they are now: a change that
    /// changed nothing, such as a click on an annotation that didn't move it.
    pub fn forget_if_unchanged(&mut self) {
        if self.past.last() == Some(&self.done) {
            self.past.pop();
        }
    }

    /// Changes an annotation, as part of the change remembered last.
    pub fn set(&mut self, index: usize, annotation: Annotation) {
        if let Some(item) = self.done.get_mut(index) {
            *item = annotation;
        }
    }

    /// Takes an annotation out, as part of the change remembered last.
    pub fn take(&mut self, index: usize) -> Option<Annotation> {
        (index < self.done.len()).then(|| self.done.remove(index))
    }

    /// Puts an annotation in at its place among the others, as part of the change remembered last.
    pub fn insert(&mut self, index: usize, annotation: Annotation) {
        self.done.insert(index.min(self.done.len()), annotation);
    }

    /// Takes the last change back; whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(before) = self.past.pop() else { return false };
        self.future.push(std::mem::replace(&mut self.done, before));
        true
    }

    /// Brings the last change taken back again; whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(after) = self.future.pop() else { return false };
        self.past.push(std::mem::replace(&mut self.done, after));
        true
    }

    pub fn clear(&mut self) {
        self.done.clear();
        self.past.clear();
        self.future.clear();
    }
}

/// The end of a line from `from` towards `to`, turned to the nearest multiple of 45°.
fn snap_angle(from: PointF, to: PointF) -> PointF {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    let length = dx.hypot(dy);
    PointF::new(from.x + length * angle.cos(), from.y + length * angle.sin())
}

/// The corner opposite `from` of the square that reaches towards `to`.
fn square(from: PointF, to: PointF) -> PointF {
    let side = (to.x - from.x).abs().max((to.y - from.y).abs());
    PointF::new(from.x + side.copysign(to.x - from.x), from.y + side.copysign(to.y - from.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = [255, 0, 0, 255];

    fn draw(tool: Tool, points: &[(f32, f32)], straight: bool) -> Option<Annotation> {
        let mut draft = Draft::start(tool, PointF::new(points[0].0, points[0].1), RED, 4.0).unwrap();
        for &(x, y) in &points[1..] {
            draft.extend(PointF::new(x, y), straight);
        }
        draft.finish(false)
    }

    #[test]
    fn a_click_with_the_line_draws_nothing() {
        assert_eq!(draw(Tool::Line, &[(10.0, 10.0), (10.5, 10.0)], false), None);
    }

    #[test]
    fn shift_keeps_a_line_at_45_degrees() {
        let line = draw(Tool::Line, &[(0.0, 0.0), (100.0, 90.0)], true).unwrap();
        let Shape::Line(_, end) = line.shape else { panic!("not a line") };
        assert!((end.x - end.y).abs() < 0.01);
    }

    #[test]
    fn shift_makes_a_rectangle_square() {
        let frame = draw(Tool::Rectangle, &[(10.0, 10.0), (60.0, -20.0)], true).unwrap();
        assert_eq!(frame.shape, Shape::Rectangle(PointF::new(10.0, 10.0), PointF::new(60.0, -40.0)));
    }

    #[test]
    fn the_brush_is_wide_and_translucent() {
        let brush = draw(Tool::Brush, &[(0.0, 0.0), (50.0, 0.0)], false).unwrap();
        assert_eq!(brush.width, 12.0);
        assert!(brush.color[3] < 255);
    }

    #[test]
    fn the_text_tool_writes_rather_than_draws() {
        assert!(Draft::start(Tool::Text, PointF::new(10.0, 10.0), RED, 4.0).is_none());
        assert_eq!(text_size(4.0), 20.0);
        assert_eq!(shadow(20.0), (2, 1));
        assert_eq!(text_margin(20.0), 6);
    }

    #[test]
    fn undo_takes_back_and_something_new_forgets_the_rest() {
        let mut history = History::default();
        let line = draw(Tool::Line, &[(0.0, 0.0), (50.0, 0.0)], false).unwrap();
        history.push(line.clone());
        history.push(line.clone());
        assert!(history.undo());
        assert_eq!(history.items().len(), 1);
        history.push(line);
        assert!(!history.redo());
        assert_eq!(history.items().len(), 2);
    }

    #[test]
    fn the_tip_of_an_arrow_is_dragged_and_undo_puts_it_back() {
        let arrow = draw(Tool::Arrow, &[(10.0, 10.0), (110.0, 10.0)], false).unwrap();
        assert_eq!(arrow.handle_at(PointF::new(108.0, 12.0), 6.0), Some(1));
        assert_eq!(arrow.handle_at(PointF::new(60.0, 10.0), 6.0), None);
        let mut history = History::default();
        history.push(arrow.clone());
        history.remember();
        history.set(0, arrow.reshaped(1, PointF::new(110.0, 60.0), false));
        assert_eq!(history.items()[0].shape, Shape::Arrow(PointF::new(10.0, 10.0), PointF::new(110.0, 60.0)));
        assert!(history.undo());
        assert_eq!(history.items()[0], arrow);
    }

    #[test]
    fn a_corner_of_a_rectangle_moves_while_the_opposite_one_stays() {
        let frame = draw(Tool::Rectangle, &[(10.0, 10.0), (60.0, 40.0)], false).unwrap();
        // The corners go round from the first one: top right is the second.
        let wider = frame.reshaped(1, PointF::new(90.0, 0.0), false);
        assert_eq!(wider.shape, Shape::Rectangle(PointF::new(10.0, 40.0), PointF::new(90.0, 0.0)));
    }

    #[test]
    fn an_annotation_is_touched_on_what_it_draws_and_moves_whole() {
        let frame = draw(Tool::Rectangle, &[(10.0, 10.0), (60.0, 40.0)], false).unwrap();
        assert!(frame.touches(PointF::new(35.0, 11.0), 4.0));
        assert!(!frame.touches(PointF::new(35.0, 25.0), 4.0), "the inside of a frame is free");
        let filled = draw(Tool::FilledRectangle, &[(10.0, 10.0), (60.0, 40.0)], false).unwrap();
        assert!(filled.touches(PointF::new(35.0, 25.0), 4.0));
        let moved = frame.moved(5.0, -5.0);
        assert_eq!(moved.shape, Shape::Rectangle(PointF::new(15.0, 5.0), PointF::new(65.0, 35.0)));
    }

    #[test]
    fn a_click_that_moves_nothing_leaves_nothing_to_undo() {
        let mut history = History::default();
        history.push(draw(Tool::Line, &[(0.0, 0.0), (50.0, 0.0)], false).unwrap());
        history.remember();
        history.forget_if_unchanged();
        assert!(history.undo());
        assert!(history.is_empty());
    }
}
