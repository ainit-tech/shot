//! The panels next to the selection. The main one has the tools, the colour, undo and closing;
//! the other one has what is done with the screenshot: the query buttons from the settings,
//! masking, copying the picture or its text, and saving. The settings say on which side of the
//! selection each panel stands and to which end of that side it is pressed: above and below the
//! selection a panel is a row, beside it a column. Here is where their buttons go and which one is
//! under the mouse; how they look is in `draw`.

use crate::annotation::Tool;
use crate::geometry::{Point, Rect};
use crate::settings::{Panel, Panels, Place, Side};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Tool(Tool),
    Color,
    /// Covers the private data in the screenshot.
    Mask,
    Undo,
    Copy,
    /// Copies the text in the screenshot.
    CopyText,
    Save,
    /// A query button from the settings, by its number there.
    Query(usize),
    Close,
}

// Sizes in pixels at the scale of 100%.
const BUTTON: f32 = 32.0;
const PADDING: f32 = 4.0;
const SEPARATOR: f32 = 9.0;
/// The distance from the selection, between two panels, and between a panel and what opens next
/// to it.
const MARGIN: f32 = 8.0;
const SWATCH: f32 = 26.0;

pub struct Toolbar {
    pub rect: Rect,
    pub buttons: Vec<(Button, Rect)>,
    pub separators: Vec<Rect>,
    pub scale: f32,
    /// Whether the buttons stand in a column.
    pub column: bool,
    /// The side of the toolbar away from the selection, where hints and the palette open first.
    away: Side,
    monitor: Rect,
}

impl Toolbar {
    /// The main toolbar and that of the actions, where the settings put them around the
    /// selection. `monitor_at` gives the monitor and its scale at a point: a panel is laid out for
    /// the monitor at the corner of the selection it is pressed to. The query buttons come in
    /// groups by their numbers in the settings: those that ask the AI, then those that search by
    /// image.
    pub fn panels(
        selection: Rect,
        settings: &Panels,
        queries: &[Vec<usize>],
        monitor_at: impl Fn(Point) -> (Rect, f32),
    ) -> (Toolbar, Toolbar) {
        let tools = vec![
            Tool::ALL.iter().map(|&tool| Button::Tool(tool)).collect(),
            vec![Button::Color, Button::Undo],
            vec![Button::Close],
        ];
        // The query buttons come first. Copying and saving come last, at the end that meets the
        // main panel in the corner by default, and masking just before them: it hides what is
        // private in the picture that then goes into the clipboard, a file or to Google.
        let mut actions: Vec<Vec<Button>> = queries
            .iter()
            .filter(|group| !group.is_empty())
            .map(|group| group.iter().map(|&i| Button::Query(i)).collect())
            .collect();
        actions.push(vec![Button::Mask]);
        actions.push(vec![Button::Copy, Button::CopyText, Button::Save]);
        let mut main = Layout::new(tools, settings.tools, selection, &monitor_at);
        let mut actions = Layout::new(actions, settings.queries, selection, &monitor_at);
        match settings.first {
            Panel::Tools => line_up(&mut main, &mut actions, selection),
            Panel::Queries => line_up(&mut actions, &mut main, selection),
        }
        (main.toolbar(), actions.toolbar())
    }

    /// Lays the groups of buttons out in a row or a column inside `rect`, with separators
    /// between the groups.
    fn place(groups: &[Vec<Button>], rect: Rect, column: bool, scale: f32, away: Side, monitor: Rect) -> Toolbar {
        let (button, padding, separator) = (px(BUTTON, scale), px(PADDING, scale), px(SEPARATOR, scale));
        let (mut x, mut y) = (rect.left + padding, rect.top + padding);
        let mut buttons = Vec::new();
        let mut separators = Vec::new();
        for (i, group) in groups.iter().enumerate() {
            if i > 0 {
                if column {
                    separators.push(Rect::new(x, y, x + button, y + separator));
                    y += separator;
                } else {
                    separators.push(Rect::new(x, y, x + separator, y + button));
                    x += separator;
                }
            }
            for &b in group {
                buttons.push((b, Rect::new(x, y, x + button, y + button)));
                if column {
                    y += button;
                } else {
                    x += button;
                }
            }
        }
        Toolbar { rect, buttons, separators, scale, column, away, monitor }
    }

    pub fn hit(&self, p: Point) -> Option<Button> {
        self.buttons.iter().find(|(_, rect)| rect.contains(p)).map(|(button, _)| *button)
    }

    pub fn button(&self, button: Button) -> Option<Rect> {
        self.buttons.iter().find(|(b, _)| *b == button).map(|(_, rect)| *rect)
    }

    /// Where something of the given size opens next to a button: on the side of the toolbar
    /// away from the selection, or on the other side when there is no room, and centred on the
    /// button along the toolbar.
    pub fn popup(&self, anchor: Rect, size: (i32, i32)) -> Rect {
        let gap = (MARGIN * self.scale).round() as i32 / 2;
        let m = self.monitor;
        let (left, top) = match self.away {
            Side::Bottom | Side::Top => {
                let left = (anchor.left + anchor.width() / 2 - size.0 / 2).min(m.right - size.0).max(m.left);
                let below = self.rect.bottom + gap;
                let above = self.rect.top - gap - size.1;
                let fits_below = below + size.1 <= m.bottom;
                let fits_above = above >= m.top;
                let top = match self.away {
                    Side::Bottom if fits_below || !fits_above => below,
                    Side::Top if !fits_above && fits_below => below,
                    _ => above,
                };
                (left, top)
            }
            Side::Right | Side::Left => {
                let top = (anchor.top + anchor.height() / 2 - size.1 / 2).min(m.bottom - size.1).max(m.top);
                let right = self.rect.right + gap;
                let left = self.rect.left - gap - size.0;
                let fits_right = right + size.0 <= m.right;
                let fits_left = left >= m.left;
                let left = match self.away {
                    Side::Right if fits_right || !fits_left => right,
                    Side::Left if !fits_left && fits_right => right,
                    _ => left,
                };
                (left, top)
            }
        };
        Rect::new(left, top, left + size.0, top + size.1)
    }
}

/// A panel being laid out: its buttons, where the settings put it, and where it goes.
struct Layout {
    groups: Vec<Vec<Button>>,
    place: Place,
    /// The side it stands on: its own, or the opposite one when there is no room on its own.
    side: Side,
    /// Whether it stands inside the selection's edge, with no room on either side.
    inside: bool,
    monitor: Rect,
    scale: f32,
    /// Its length along the side.
    length: i32,
    /// Where it begins along the side, and across it.
    along: i32,
    across: i32,
}

impl Layout {
    fn new(groups: Vec<Vec<Button>>, place: Place, selection: Rect, monitor_at: &impl Fn(Point) -> (Rect, f32)) -> Layout {
        let (monitor, scale) = monitor_at(corner(selection, place));
        let (thick, margin) = (thickness(scale), px(MARGIN, scale));
        let room = |side: Side| match side {
            Side::Bottom => selection.bottom + margin + thick <= monitor.bottom,
            Side::Top => selection.top - margin - thick >= monitor.top,
            Side::Right => selection.right + margin + thick <= monitor.right,
            Side::Left => selection.left - margin - thick >= monitor.left,
        };
        let (side, inside) = match (room(place.side), room(place.side.opposite())) {
            (true, _) => (place.side, false),
            (false, true) => (place.side.opposite(), false),
            (false, false) => (place.side, true),
        };
        let across = match (side, inside) {
            (Side::Bottom, false) => selection.bottom + margin,
            (Side::Bottom, true) => (selection.bottom - margin - thick).max(monitor.top),
            (Side::Top, false) => selection.top - margin - thick,
            (Side::Top, true) => (selection.top + margin).min(monitor.bottom - thick),
            (Side::Right, false) => selection.right + margin,
            (Side::Right, true) => (selection.right - margin - thick).max(monitor.left),
            (Side::Left, false) => selection.left - margin - thick,
            (Side::Left, true) => (selection.left + margin).min(monitor.right - thick),
        };
        let length = length(&groups, scale);
        let (start, end) = span(selection, side);
        let along = if place.at_end() { end - length } else { start };
        Layout { groups, place, side, inside, monitor, scale, length, along, across }
    }

    /// Where the panel is, kept on its monitor along its side.
    fn rect(&self) -> Rect {
        let (low, high) = span(self.monitor, self.side);
        let along = self.along.min(high - self.length).max(low);
        let thick = thickness(self.scale);
        match self.side.upright() {
            true => Rect::new(self.across, along, self.across + thick, along + self.length),
            false => Rect::new(along, self.across, along + self.length, self.across + thick),
        }
    }

    fn toolbar(self) -> Toolbar {
        // Inside the selection, what opens next to the panel opens into the selection.
        let away = if self.inside { self.side.opposite() } else { self.side };
        Toolbar::place(&self.groups, self.rect(), self.side.upright(), self.scale, away, self.monitor)
    }
}

/// Two panels on one side that would overlap stand in a row, the first one first, pressed to the
/// end the first one is pressed to. A panel that would cover the other at a corner moves along
/// its side, away from its end.
fn line_up(first: &mut Layout, second: &mut Layout, selection: Rect) {
    let gap = px(MARGIN, first.scale);
    if first.side == second.side && first.inside == second.inside {
        let apart = first.along + first.length + gap <= second.along || second.along + second.length + gap <= first.along;
        if apart {
            return;
        }
        let total = first.length + gap + second.length;
        let (start, end) = span(selection, first.side);
        let (low, high) = span(first.monitor, first.side);
        let row = if first.place.at_end() { end - total } else { start };
        first.along = row.min(high - total).max(low);
        second.along = first.along + first.length + gap;
        return;
    }
    let (kept, moved) = (first.rect(), second.rect());
    if kept.intersect(moved).is_empty() {
        return;
    }
    let (from, to) = span(kept, second.side);
    second.along = if second.place.at_end() { from - gap - second.length } else { to + gap };
}

/// The corner of the selection at the end of the side a panel is pressed to.
fn corner(selection: Rect, place: Place) -> Point {
    let (right, bottom) = (selection.right - 1, selection.bottom - 1);
    match place.side {
        Side::Bottom => Point::new(if place.at_end() { right } else { selection.left }, bottom),
        Side::Top => Point::new(if place.at_end() { right } else { selection.left }, selection.top),
        Side::Left => Point::new(selection.left, if place.at_end() { bottom } else { selection.top }),
        Side::Right => Point::new(right, if place.at_end() { bottom } else { selection.top }),
    }
}

/// Where a rectangle begins and ends along a side: left and right for a row above or below it,
/// top and bottom for a column beside it.
fn span(rect: Rect, side: Side) -> (i32, i32) {
    if side.upright() { (rect.top, rect.bottom) } else { (rect.left, rect.right) }
}

/// How thick a panel is: one button with its padding.
fn thickness(scale: f32) -> i32 {
    px(BUTTON, scale) + 2 * px(PADDING, scale)
}

/// The length of a row or a column of groups of buttons, with its padding and separators.
fn length(groups: &[Vec<Button>], scale: f32) -> i32 {
    let count: usize = groups.iter().map(Vec::len).sum();
    2 * px(PADDING, scale) + count as i32 * px(BUTTON, scale) + (groups.len() as i32 - 1) * px(SEPARATOR, scale)
}

fn px(size: f32, scale: f32) -> i32 {
    (size * scale).round() as i32
}

/// The colours that open from the colour button: a row next to a row of buttons, a column next
/// to a column.
pub struct Palette {
    pub rect: Rect,
    pub swatches: Vec<Rect>,
}

impl Palette {
    pub fn layout(toolbar: &Toolbar, colors: usize) -> Option<Palette> {
        let anchor = toolbar.button(Button::Color)?;
        let (swatch, padding) = (px(SWATCH, toolbar.scale), px(PADDING, toolbar.scale) + 2);
        let (long, short) = (2 * padding + colors as i32 * swatch, 2 * padding + swatch);
        let size = if toolbar.column { (short, long) } else { (long, short) };
        let rect = toolbar.popup(anchor, size);
        let swatches = (0..colors as i32)
            .map(|i| {
                let (left, top) = match toolbar.column {
                    true => (rect.left + padding, rect.top + padding + i * swatch),
                    false => (rect.left + padding + i * swatch, rect.top + padding),
                };
                Rect::new(left, top, left + swatch, top + swatch)
            })
            .collect();
        Some(Palette { rect, swatches })
    }

    pub fn hit(&self, p: Point) -> Option<usize> {
        self.swatches.iter().position(|rect| rect.contains(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Align;

    const MONITOR: Rect = Rect::new(0, 0, 1920, 1080);
    const SELECTION: Rect = Rect::new(600, 200, 1400, 700);

    fn queries() -> Vec<Vec<usize>> {
        vec![vec![0, 1, 2], vec![3, 4]]
    }

    fn panels(selection: Rect, settings: &Panels) -> (Toolbar, Toolbar) {
        Toolbar::panels(selection, settings, &queries(), |_| (MONITOR, 1.0))
    }

    fn place(side: Side, align: Align) -> Place {
        Place { side, align }
    }

    #[test]
    fn the_main_toolbar_goes_under_the_selection_at_its_right_edge() {
        let (toolbar, _) = panels(SELECTION, &Panels::default());
        assert_eq!(toolbar.rect.right, 1400);
        assert_eq!(toolbar.rect.top, 708);
        assert!(!toolbar.column);
        assert_eq!(toolbar.hit(Point::new(1400 - 20, 708 + 20)), Some(Button::Close));
        assert_eq!(toolbar.buttons.len(), 8 + 2 + 1);
    }

    #[test]
    fn the_column_asks_first_then_masks_copies_and_saves() {
        let (_, toolbar) = panels(SELECTION, &Panels::default());
        let order: Vec<Button> = toolbar.buttons.iter().map(|&(button, _)| button).collect();
        let actions = [Button::Mask, Button::Copy, Button::CopyText, Button::Save];
        let expected: Vec<Button> = (0..5).map(Button::Query).chain(actions).collect();
        assert_eq!(order, expected);
        assert_eq!(toolbar.separators.len(), 3);
    }

    #[test]
    fn without_room_below_the_main_toolbar_goes_above_and_stays_on_the_monitor() {
        let (toolbar, _) = panels(Rect::new(10, 300, 200, 1075), &Panels::default());
        assert!(toolbar.rect.bottom <= 300);
        assert_eq!(toolbar.rect.left, 0);
    }

    #[test]
    fn the_query_buttons_stand_in_a_column_right_of_the_selection_at_its_bottom() {
        let (_, toolbar) = panels(SELECTION, &Panels::default());
        assert_eq!((toolbar.rect.left, toolbar.rect.bottom), (1408, 700));
        assert_eq!(toolbar.rect.width(), 40);
        assert!(toolbar.column);
        let (first, last) = (toolbar.button(Button::Query(0)).unwrap(), toolbar.button(Button::Query(4)).unwrap());
        assert_eq!(first.left, last.left);
        assert!(last.top > first.top);
    }

    #[test]
    fn without_room_on_the_right_the_column_goes_left_of_the_selection() {
        let (_, toolbar) = panels(Rect::new(600, 200, 1900, 700), &Panels::default());
        assert_eq!(toolbar.rect.right, 592);
    }

    #[test]
    fn the_ai_and_the_image_search_are_separate_groups() {
        let (_, toolbar) = panels(SELECTION, &Panels::default());
        let ai = toolbar.button(Button::Query(2)).unwrap();
        let images = toolbar.button(Button::Query(3)).unwrap();
        assert!(toolbar.separators.iter().any(|s| s.top >= ai.bottom && s.bottom <= images.top));
    }

    #[test]
    fn hints_of_the_column_open_beside_it() {
        let (_, toolbar) = panels(SELECTION, &Panels::default());
        let anchor = toolbar.button(Button::Query(0)).unwrap();
        let hint = toolbar.popup(anchor, (150, 22));
        assert_eq!(hint.left, toolbar.rect.right + 4);
        assert_eq!(hint.top + 11, anchor.top + anchor.height() / 2);
    }

    #[test]
    fn without_query_buttons_the_column_has_the_actions_alone() {
        let (_, toolbar) = Toolbar::panels(SELECTION, &Panels::default(), &[vec![], vec![]], |_| (MONITOR, 1.0));
        let order: Vec<Button> = toolbar.buttons.iter().map(|&(button, _)| button).collect();
        assert_eq!(order, [Button::Mask, Button::Copy, Button::CopyText, Button::Save]);
        assert_eq!(toolbar.separators.len(), 1);
    }

    #[test]
    fn beside_the_selection_a_panel_is_a_column_and_above_it_a_row() {
        let settings = Panels { tools: place(Side::Left, Align::Top), queries: place(Side::Top, Align::Left), ..Panels::default() };
        let (main, search) = panels(SELECTION, &settings);
        assert!(main.column && !search.column);
        assert_eq!((main.rect.right, main.rect.top), (592, 200));
        assert_eq!((search.rect.left, search.rect.bottom), (600, 192));
    }

    #[test]
    fn two_panels_pressed_to_one_end_stand_in_a_row_in_their_order() {
        let both = place(Side::Bottom, Align::Right);
        let settings = Panels { tools: both, queries: both, first: Panel::Tools };
        let (main, search) = panels(SELECTION, &settings);
        assert_eq!((search.rect.right, search.rect.top, main.rect.top), (1400, 708, 708));
        assert_eq!(main.rect.right + 8, search.rect.left);
        let (main, search) = panels(SELECTION, &Panels { first: Panel::Queries, ..settings });
        assert_eq!(main.rect.right, 1400);
        assert_eq!(search.rect.right + 8, main.rect.left);
    }

    #[test]
    fn panels_inside_a_selection_of_the_whole_screen_dont_cover_each_other() {
        let (main, search) = panels(MONITOR, &Panels::default());
        assert!(main.rect.bottom <= 1080 && search.rect.right <= 1920);
        assert!(main.rect.intersect(search.rect).is_empty());
        // What opens next to them opens into the selection.
        let hint = main.popup(main.button(Button::Close).unwrap(), (100, 22));
        assert!(hint.bottom <= main.rect.top);
    }

    #[test]
    fn the_palette_of_a_column_is_a_column_beside_it() {
        let settings = Panels { tools: place(Side::Right, Align::Top), ..Panels::default() };
        let (main, _) = Toolbar::panels(SELECTION, &settings, &[], |_| (MONITOR, 1.0));
        let palette = Palette::layout(&main, 8).unwrap();
        assert!(palette.rect.height() > palette.rect.width());
        assert_eq!(palette.rect.left, main.rect.right + 4);
        assert_eq!(palette.swatches[0].left, palette.swatches[7].left);
    }
}
