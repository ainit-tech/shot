//! The selection: the part of the frozen screen the user works with. The mouse draws it,
//! drags it from inside and resizes it by its edges and corners.

use crate::geometry::{Point, Rect};

/// Which edges of the selection a drag moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub left: bool,
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
}

impl Edges {
    fn any(&self) -> bool {
        self.left || self.top || self.right || self.bottom
    }
}

/// What part of the selection is under the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Outside,
    Inside,
    Edges(Edges),
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    None,
    Drawing { anchor: Point },
    Moving { grab: Point, start: Rect },
    Resizing { edges: Edges, grab: Point, start: Rect },
}

pub struct Selection {
    rect: Option<Rect>,
    drag: Drag,
    bounds: Rect,
}

impl Selection {
    /// No selection yet on a screen of the given size.
    pub fn new(bounds: Rect) -> Self {
        Self { rect: None, drag: Drag::None, bounds }
    }

    /// The selection; while an edge is dragged onto the opposite one, it can be empty.
    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    pub fn is_dragging(&self) -> bool {
        !matches!(self.drag, Drag::None)
    }

    /// What is under the point. An edge takes the mouse within `reach` pixels on either side.
    pub fn hit(&self, p: Point, reach: i32) -> Hit {
        let Some(r) = self.rect else { return Hit::Outside };
        if !r.inflate(reach).contains(p) {
            return Hit::Outside;
        }
        let near = |a: i32, b: i32| (a - b).abs() <= reach;
        let mut edges = Edges {
            left: near(p.x, r.left),
            top: near(p.y, r.top),
            right: near(p.x, r.right),
            bottom: near(p.y, r.bottom),
        };
        // In a narrow selection both opposite edges can be near; the closer one wins.
        if edges.left && edges.right {
            if (p.x - r.left).abs() <= (p.x - r.right).abs() {
                edges.right = false;
            } else {
                edges.left = false;
            }
        }
        if edges.top && edges.bottom {
            if (p.y - r.top).abs() <= (p.y - r.bottom).abs() {
                edges.bottom = false;
            } else {
                edges.top = false;
            }
        }
        if edges.any() {
            Hit::Edges(edges)
        } else if r.contains(p) {
            Hit::Inside
        } else {
            Hit::Outside
        }
    }

    /// The mouse button goes down: inside, the selection is moved; at an edge, resized;
    /// outside, a new one is drawn.
    pub fn press(&mut self, p: Point, reach: i32) {
        self.drag = match (self.hit(p, reach), self.rect) {
            (Hit::Inside, Some(start)) => Drag::Moving { grab: p, start },
            (Hit::Edges(edges), Some(start)) => Drag::Resizing { edges, grab: p, start },
            _ => {
                self.rect = None;
                Drag::Drawing { anchor: self.clamp(p) }
            }
        };
    }

    /// The mouse moves with the button down.
    pub fn drag_to(&mut self, p: Point) {
        let p = self.clamp(p);
        match self.drag {
            Drag::None => {}
            Drag::Drawing { anchor } => {
                let rect = Rect::spanning(anchor, p);
                self.rect = (!rect.is_empty()).then_some(rect);
            }
            Drag::Moving { grab, start } => {
                let moved = start.offset(p.x - grab.x, p.y - grab.y);
                self.rect = Some(moved.moved_inside(self.bounds));
            }
            Drag::Resizing { edges, grab, start } => {
                let (dx, dy) = (p.x - grab.x, p.y - grab.y);
                let mut a = start.top_left();
                let mut b = Point::new(start.right, start.bottom);
                if edges.left {
                    a.x += dx;
                }
                if edges.top {
                    a.y += dy;
                }
                if edges.right {
                    b.x += dx;
                }
                if edges.bottom {
                    b.y += dy;
                }
                // An edge dragged past the opposite one turns the selection over.
                self.rect = Some(Rect::spanning(self.clamp(a), self.clamp(b)));
            }
        }
    }

    /// The mouse button goes up.
    pub fn release(&mut self) {
        self.drag = Drag::None;
        if self.rect.is_some_and(|r| r.is_empty()) {
            self.rect = None;
        }
    }

    /// Selects the given rectangle, for example a whole monitor.
    pub fn select(&mut self, rect: Rect) {
        let rect = rect.intersect(self.bounds);
        self.rect = (!rect.is_empty()).then_some(rect);
        self.drag = Drag::None;
    }

    pub fn clear(&mut self) {
        self.rect = None;
        self.drag = Drag::None;
    }

    /// The nearest point on the screen; its right and bottom edges count.
    fn clamp(&self, p: Point) -> Point {
        Point::new(
            p.x.clamp(self.bounds.left, self.bounds.right),
            p.y.clamp(self.bounds.top, self.bounds.bottom),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 1000, 800);
    const REACH: i32 = 6;

    fn selected(rect: Rect) -> Selection {
        let mut selection = Selection::new(SCREEN);
        selection.select(rect);
        selection
    }

    fn drag(selection: &mut Selection, from: (i32, i32), to: (i32, i32)) {
        selection.press(Point::new(from.0, from.1), REACH);
        selection.drag_to(Point::new(to.0, to.1));
        selection.release();
    }

    #[test]
    fn drawing_selects_between_the_press_and_the_release_in_any_direction() {
        let mut selection = Selection::new(SCREEN);
        drag(&mut selection, (300, 200), (100, 150));
        assert_eq!(selection.rect(), Some(Rect::new(100, 150, 300, 200)));
    }

    #[test]
    fn a_click_without_dragging_selects_nothing() {
        let mut selection = selected(Rect::new(100, 100, 300, 200));
        drag(&mut selection, (500, 500), (500, 500));
        assert_eq!(selection.rect(), None);
    }

    #[test]
    fn drawing_stops_at_the_edge_of_the_screen() {
        let mut selection = Selection::new(SCREEN);
        drag(&mut selection, (900, 700), (1200, 900));
        assert_eq!(selection.rect(), Some(Rect::new(900, 700, 1000, 800)));
    }

    #[test]
    fn a_moved_selection_stays_on_the_screen() {
        let mut selection = selected(Rect::new(100, 100, 300, 200));
        drag(&mut selection, (150, 150), (1100, 150));
        assert_eq!(selection.rect(), Some(Rect::new(800, 100, 1000, 200)));
    }

    #[test]
    fn an_edge_dragged_past_the_opposite_one_turns_the_selection_over() {
        let mut selection = selected(Rect::new(100, 100, 300, 200));
        drag(&mut selection, (100, 150), (400, 150));
        assert_eq!(selection.rect(), Some(Rect::new(300, 100, 400, 200)));
    }

    #[test]
    fn a_corner_moves_two_edges() {
        let mut selection = selected(Rect::new(100, 100, 300, 200));
        assert_eq!(
            selection.hit(Point::new(302, 198), REACH),
            Hit::Edges(Edges { right: true, bottom: true, ..Edges::default() })
        );
        drag(&mut selection, (302, 198), (402, 248));
        assert_eq!(selection.rect(), Some(Rect::new(100, 100, 400, 250)));
    }

    #[test]
    fn in_a_narrow_selection_the_closer_edge_is_taken() {
        let selection = selected(Rect::new(100, 100, 104, 200));
        assert_eq!(
            selection.hit(Point::new(103, 150), REACH),
            Hit::Edges(Edges { right: true, ..Edges::default() })
        );
    }
}
