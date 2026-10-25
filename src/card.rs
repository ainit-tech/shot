//! The answer card and the conversation window (decision 8): how big they are and where they go
//! next to the selection. The card is as wide as its longest line, from a width that fits a
//! short answer up to one that is still comfortable to read, and as tall as its text, up to a
//! height past which the text scrolls. The window has a size of its own, for a conversation or
//! for the results of the image search. Both go to the right of the selection, or to the left of
//! it, or under or above it, and never leave the screen. Lengths here are at the scale of 100%;
//! `scale` turns them into pixels.

use crate::geometry::Rect;

/// The narrowest card, wide enough for a short answer and the link under it.
const MIN_WIDTH: i32 = 240;
/// The widest card: lines longer than about seventy letters are hard to read.
const MAX_WIDTH: i32 = 560;
/// The tallest card; past it the text scrolls.
const MAX_HEIGHT: i32 = 480;
/// The part of the screen's height the card may take at most.
const MAX_SHARE: f32 = 0.6;
/// The space between the selection and the card, and between the card and the screen's edges.
const GAP: i32 = 8;
/// The window with a conversation: wide enough for AI Mode's answers, with their lists and
/// sources, and tall enough for a long answer with the question field under it.
pub const CONVERSATION: (i32, i32) = (600, 760);
/// The window with the results of the image search: as wide as Google's page of results needs.
pub const RESULTS: (i32, i32) = (720, 760);

fn px(length: i32, scale: f32) -> i32 {
    (length as f32 * scale).round() as i32
}

/// The card's width for text whose longest line is `line` pixels wide, with `frame` pixels of
/// padding and scroll bar around it.
pub fn width(line: i32, frame: i32, scale: f32, screen: Rect) -> i32 {
    let max = px(MAX_WIDTH, scale).min(screen.width() - 2 * px(GAP, scale));
    (line + frame).clamp(px(MIN_WIDTH, scale).min(max), max)
}

/// The card's height for text `text` pixels tall with `frame` pixels of padding and the link
/// around it, and whether the text has to scroll.
pub fn height(text: i32, frame: i32, scale: f32, screen: Rect) -> (i32, bool) {
    let max = px(MAX_HEIGHT, scale).min((screen.height() as f32 * MAX_SHARE) as i32);
    let wanted = text + frame;
    (wanted.min(max), wanted > max)
}

/// The window's size, as seen, title bar included: [`CONVERSATION`] or [`RESULTS`], and on a
/// small screen as much of the screen as there is.
pub fn window_size((width, height): (i32, i32), scale: f32, screen: Rect) -> (i32, i32) {
    let room = screen.inflate(-px(GAP, scale));
    (px(width, scale).min(room.width()), px(height, scale).min(room.height()))
}

/// Where a card or a window of the given size goes: to the right of the selection, level with
/// its top, where what was asked about stays in sight; to the left of it if there is no room on
/// the right; under or above it if there is no room beside it; and in any case inside the
/// screen.
pub fn place(selection: Rect, (width, height): (i32, i32), scale: f32, screen: Rect) -> Rect {
    let gap = px(GAP, scale);
    let room = screen.inflate(-gap);
    let at = |left: i32, top: i32| Rect::new(left, top, left + width, top + height);
    let fits = |card: Rect| card.top >= room.top && card.bottom <= room.bottom && card.left >= room.left && card.right <= room.right;
    // Beside the selection the card may slide up and down, above and under it left and right.
    let upright = Rect::new(i32::MIN / 2, room.top, i32::MAX / 2, room.bottom);
    let level = Rect::new(room.left, i32::MIN / 2, room.right, i32::MAX / 2);
    let right = at(selection.right + gap, selection.top).moved_inside(upright);
    let left = at(selection.left - gap - width, selection.top).moved_inside(upright);
    let below = at(selection.left, selection.bottom + gap).moved_inside(level);
    let above = at(selection.left, selection.top - gap - height).moved_inside(level);
    [right, left, below, above].into_iter().find(|&card| fits(card)).unwrap_or(right).moved_inside(room)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { left: 0, top: 0, right: 1920, bottom: 1040 };

    #[test]
    fn the_card_is_as_wide_as_its_text_within_limits() {
        assert_eq!(width(100, 30, 1.0, SCREEN), 240, "a short answer gets the narrowest card");
        assert_eq!(width(400, 30, 1.0, SCREEN), 430);
        assert_eq!(width(2000, 30, 1.0, SCREEN), 560, "a long line wraps at a comfortable width");
        assert_eq!(width(400, 30, 2.0, SCREEN), 480, "the limits grow with the scale");
    }

    #[test]
    fn a_tall_answer_scrolls() {
        assert_eq!(height(200, 60, 1.0, SCREEN), (260, false));
        assert_eq!(height(900, 60, 1.0, SCREEN), (480, true));
        let small = Rect { left: 0, top: 0, right: 1366, bottom: 728 };
        assert_eq!(height(900, 60, 1.0, small), (436, true), "on a small screen it takes at most 60% of it");
    }

    #[test]
    fn the_window_fits_what_it_shows_or_the_screen() {
        assert_eq!(window_size(CONVERSATION, 1.0, SCREEN), (600, 760));
        assert_eq!(window_size(RESULTS, 1.0, SCREEN), (720, 760));
        assert_eq!(window_size(CONVERSATION, 1.5, SCREEN), (900, 1016), "at 150% it is as tall as the screen lets it be");
        let small = Rect { left: 0, top: 0, right: 1366, bottom: 728 };
        assert_eq!(window_size(CONVERSATION, 1.0, small), (600, 712));
    }

    #[test]
    fn the_window_goes_to_the_right_of_the_selection_as_the_card_does() {
        let window = place(Rect::new(100, 100, 600, 300), window_size(CONVERSATION, 1.0, SCREEN), 1.0, SCREEN);
        assert_eq!(window, Rect::new(608, 100, 1208, 860));
    }

    #[test]
    fn the_card_goes_to_the_right_of_the_selection() {
        let card = place(Rect::new(100, 100, 600, 300), (400, 200), 1.0, SCREEN);
        assert_eq!(card, Rect::new(608, 100, 1008, 300));
    }

    #[test]
    fn near_the_right_edge_it_goes_to_the_left() {
        let card = place(Rect::new(1500, 100, 1900, 300), (400, 200), 1.0, SCREEN);
        assert_eq!(card, Rect::new(1092, 100, 1492, 300));
    }

    #[test]
    fn near_the_bottom_it_slides_up_beside_the_selection() {
        let card = place(Rect::new(100, 900, 600, 1000), (400, 200), 1.0, SCREEN);
        assert_eq!(card, Rect::new(608, 832, 1008, 1032));
    }

    #[test]
    fn a_selection_as_wide_as_the_screen_gets_the_card_under_it() {
        let card = place(Rect::new(0, 100, 1920, 300), (400, 200), 1.0, SCREEN);
        assert_eq!(card, Rect::new(8, 308, 408, 508));
    }

    #[test]
    fn a_wide_selection_at_the_bottom_gets_the_card_above_it() {
        let card = place(Rect::new(0, 800, 1920, 1000), (400, 200), 1.0, SCREEN);
        assert_eq!(card, Rect::new(8, 592, 408, 792));
    }

    #[test]
    fn with_no_room_anywhere_it_still_stays_on_the_screen() {
        let card = place(Rect::new(0, 0, 1920, 1040), (400, 200), 1.0, SCREEN);
        assert!(card.left >= 8 && card.right <= 1912 && card.top >= 8 && card.bottom <= 1032, "{card:?}");
    }
}
