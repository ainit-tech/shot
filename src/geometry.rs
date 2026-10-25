//! Points and rectangles in pixels of the frozen screen.

/// A point in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A point in pixels with fractions, for what the mouse draws.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

impl PointF {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: PointF) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

impl From<Point> for PointF {
    fn from(p: Point) -> Self {
        PointF::new(p.x as f32, p.y as f32)
    }
}

/// A rectangle in pixels. It includes its left and top edges and ends just before its right
/// and bottom ones, so its width is `right - left`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }

    /// The rectangle between two opposite corners given in any order.
    pub fn spanning(a: Point, b: Point) -> Self {
        Self::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
    }

    /// The whole pixels that points with fractions cover, with `pad` pixels around them.
    pub fn bounding(points: impl IntoIterator<Item = PointF>, pad: f32) -> Self {
        let mut points = points.into_iter();
        let Some(first) = points.next() else { return Rect::default() };
        let (mut min, mut max) = (first, first);
        for p in points {
            min = PointF::new(min.x.min(p.x), min.y.min(p.y));
            max = PointF::new(max.x.max(p.x), max.y.max(p.y));
        }
        Rect::new(
            (min.x - pad).floor() as i32,
            (min.y - pad).floor() as i32,
            (max.x + pad).ceil() as i32 + 1,
            (max.y + pad).ceil() as i32 + 1,
        )
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn is_empty(&self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }

    pub fn top_left(&self) -> Point {
        Point::new(self.left, self.top)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.top && p.y < self.bottom
    }

    /// The common part of two rectangles; empty if they don't meet.
    pub fn intersect(&self, other: Rect) -> Rect {
        Rect::new(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right.min(other.right),
            self.bottom.min(other.bottom),
        )
    }

    /// The smallest rectangle that holds both.
    pub fn union(&self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return *self;
        }
        Rect::new(
            self.left.min(other.left),
            self.top.min(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
        )
    }

    /// The rectangle grown by `d` pixels on every side.
    pub fn inflate(&self, d: i32) -> Rect {
        Rect::new(self.left - d, self.top - d, self.right + d, self.bottom + d)
    }

    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.left + dx, self.top + dy, self.right + dx, self.bottom + dy)
    }

    /// The same rectangle moved as little as possible to lie inside `bounds`. If it is larger
    /// than `bounds`, its left and top edges stay inside.
    pub fn moved_inside(&self, bounds: Rect) -> Rect {
        let shift = |start: i32, end: i32, min: i32, max: i32| {
            if start < min { min - start } else { (max - end).min(0).max(min - start) }
        };
        self.offset(
            shift(self.left, self.right, bounds.left, bounds.right),
            shift(self.top, self.bottom, bounds.top, bounds.bottom),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_is_moved_back_inside_from_any_side() {
        let bounds = Rect::new(0, 0, 100, 100);
        assert_eq!(Rect::new(-5, 10, 15, 30).moved_inside(bounds), Rect::new(0, 10, 20, 30));
        assert_eq!(Rect::new(90, 95, 110, 105).moved_inside(bounds), Rect::new(80, 90, 100, 100));
        assert_eq!(Rect::new(10, 10, 20, 20).moved_inside(bounds), Rect::new(10, 10, 20, 20));
    }

    #[test]
    fn a_rectangle_larger_than_its_bounds_keeps_its_top_left_inside() {
        let bounds = Rect::new(0, 0, 100, 100);
        assert_eq!(Rect::new(20, 20, 220, 220).moved_inside(bounds), Rect::new(0, 0, 200, 200));
    }
}
