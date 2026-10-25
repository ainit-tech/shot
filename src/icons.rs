//! The toolbar's icons: lines on a grid of 20 by 20, the way icon sets are made, so that they
//! stay sharp at every scale. A shape is written like an SVG path, with absolute commands
//! only: `M x y`, `L x y`, `H x`, `V y`, `C x1 y1 x2 y2 x y` and `Z`.

use tiny_skia::{Path, PathBuilder};

/// One part of an icon.
pub enum Part {
    /// An outline drawn with the icon's line.
    Line(&'static str),
    /// A shape filled with the icon's colour.
    Fill(&'static str),
    /// A filled circle: its centre and radius.
    Dot(f32, f32, f32),
}

/// The size of the grid the icons are drawn on, and the width of their lines on it.
pub const GRID: f32 = 20.0;
pub const LINE: f32 = 1.5;

pub const PENCIL: &[Part] = &[Part::Line("M 13.5 3.5 L 16.5 6.5 L 7 16 L 3.5 16.5 L 4 13 Z M 11.5 5.5 L 14.5 8.5")];

pub const BRUSH: &[Part] = &[
    Part::Line("M 16.5 3.5 L 10.5 9.5"),
    Part::Fill("M 10.5 9.5 C 12 11 11.5 13 10 14.5 C 8.5 16 6 16.5 3.5 16.5 C 3.5 14 4 11.5 5.5 10 C 7 8.5 9 8 10.5 9.5 Z"),
];

pub const LINE_TOOL: &[Part] = &[Part::Line("M 4 16 L 16 4")];

pub const ARROW: &[Part] = &[Part::Line("M 4 16 L 15.5 4.5 M 8.5 4.5 L 15.5 4.5 L 15.5 11.5")];

const BOX: &str = "M 5 4.5 H 15 C 16 4.5 16.5 5 16.5 6 V 14 C 16.5 15 16 15.5 15 15.5 H 5 C 4 15.5 3.5 15 3.5 14 V 6 C 3.5 5 4 4.5 5 4.5 Z";

pub const RECTANGLE: &[Part] = &[Part::Line(BOX)];

pub const FILLED_RECTANGLE: &[Part] = &[Part::Fill(BOX)];

pub const MAGIC_PENCIL: &[Part] = &[
    Part::Line("M 2.5 11.5 C 2.5 9 5.4 7 9 7 C 12.6 7 15.5 9 15.5 11.5 C 15.5 14 12.6 16 9 16 C 5.4 16 2.5 14 2.5 11.5 Z"),
    Part::Fill("M 15.5 1.5 L 16.4 3.6 L 18.5 4.5 L 16.4 5.4 L 15.5 7.5 L 14.6 5.4 L 12.5 4.5 L 14.6 3.6 Z"),
];

/// Masking private data: lines of text, one word of them blacked out.
pub const MASK: &[Part] = &[
    Part::Line("M 3.5 4.5 H 16.5 M 14.5 10 H 16.5 M 3.5 15.5 H 11"),
    Part::Fill("M 4.5 8 H 11.5 C 12.1 8 12.5 8.4 12.5 9 V 11 C 12.5 11.6 12.1 12 11.5 12 H 4.5 C 3.9 12 3.5 11.6 3.5 11 V 9 C 3.5 8.4 3.9 8 4.5 8 Z"),
];

pub const UNDO: &[Part] = &[Part::Line(
    "M 7.5 4.5 L 4 8 L 7.5 11.5 M 4 8 H 12 C 14.5 8 16.5 10 16.5 12.5 C 16.5 15 14.5 16 12 16 H 8",
)];

pub const COPY: &[Part] = &[
    Part::Line("M 4.5 12.5 C 3.9 12.5 3.5 12.1 3.5 11.5 V 5 C 3.5 4.2 4.2 3.5 5 3.5 H 11.5 C 12.1 3.5 12.5 3.9 12.5 4.5"),
    Part::Line("M 8.5 7 H 15 C 15.8 7 16.5 7.7 16.5 8.5 V 15 C 16.5 15.8 15.8 16.5 15 16.5 H 8.5 C 7.7 16.5 7 15.8 7 15 V 8.5 C 7 7.7 7.7 7 8.5 7 Z"),
];

pub const SAVE: &[Part] = &[Part::Line(
    "M 10 3.5 V 12.5 M 6.5 9 L 10 12.5 L 13.5 9 M 4 13.5 V 15 C 4 15.8 4.7 16.5 5.5 16.5 H 14.5 C 15.3 16.5 16 15.8 16 15 V 13.5",
)];

pub const CLOSE: &[Part] = &[Part::Line("M 5.5 5.5 L 14.5 14.5 M 14.5 5.5 L 5.5 14.5")];

pub const TEXT: &[Part] = &[Part::Line("M 4.5 6 V 4 H 15.5 V 6 M 10 4 V 16 M 7.5 16 H 12.5")];

/// Copying the text of the screenshot: a page with lines of text.
pub const TEXT_COPY: &[Part] = &[Part::Line(
    "M 6 3.5 H 14 C 15.1 3.5 16 4.4 16 5.5 V 14.5 C 16 15.6 15.1 16.5 14 16.5 H 6 C 4.9 16.5 4 15.6 4 14.5 V 5.5 C 4 4.4 4.9 3.5 6 3.5 Z M 7 7.5 H 13 M 7 10.5 H 13 M 7 13.5 H 10.5",
)];

pub const IMAGES: &[Part] = &[
    Part::Line("M 5 4 H 15 C 16.1 4 17 4.9 17 6 V 14 C 17 15.1 16.1 16 15 16 H 5 C 3.9 16 3 15.1 3 14 V 6 C 3 4.9 3.9 4 5 4 Z"),
    Part::Line("M 3.5 14 L 8 9.5 L 11.5 13 L 13.5 11 L 16.5 14"),
    Part::Dot(13.0, 7.5, 1.4),
];

pub const QUESTION: &[Part] = &[
    Part::Line("M 7.5 7.5 C 7.5 5.6 8.9 4.5 10.2 4.5 C 11.7 4.5 12.8 5.6 12.8 7 C 12.8 9 10.2 9.4 10.2 11.8"),
    Part::Dot(10.2, 14.8, 1.0),
];

/// The shape written in the path language above.
pub fn path(d: &str) -> Option<Path> {
    let mut words = d.split_whitespace();
    let mut path = PathBuilder::new();
    let mut at = (0.0, 0.0);
    while let Some(command) = words.next() {
        // Every command takes its numbers from the words that follow it.
        let mut take = || words.next()?.parse::<f32>().ok();
        match command {
            "M" => {
                at = (take()?, take()?);
                path.move_to(at.0, at.1);
            }
            "L" => {
                at = (take()?, take()?);
                path.line_to(at.0, at.1);
            }
            "H" => {
                at.0 = take()?;
                path.line_to(at.0, at.1);
            }
            "V" => {
                at.1 = take()?;
                path.line_to(at.0, at.1);
            }
            "C" => {
                let (x1, y1, x2, y2) = (take()?, take()?, take()?, take()?);
                at = (take()?, take()?);
                path.cubic_to(x1, y1, x2, y2, at.0, at.1);
            }
            "Z" => path.close(),
            _ => return None,
        }
    }
    path.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_reads() {
        let icons = [
            PENCIL, BRUSH, LINE_TOOL, ARROW, RECTANGLE, FILLED_RECTANGLE, MAGIC_PENCIL, MASK, UNDO, COPY, SAVE, CLOSE,
            IMAGES, QUESTION,
        ];
        for icon in icons {
            for part in icon {
                if let Part::Line(d) | Part::Fill(d) = part {
                    assert!(path(d).is_some(), "{d}");
                }
            }
        }
    }
}
