//! The marks of the mouse's clicks in a recording (decision 17). From a press a thin ring goes out
//! and fades, and a soft dot lies under the pointer for as long as the button is held. They are in
//! the picture of the recording alone: the screen stays as it is.

/// How long a ring lasts, and how long a dot takes to come and to go, in seconds.
const RING_TIME: f32 = 0.5;
const DOT_COMES: f32 = 0.06;
const DOT_GOES: f32 = 0.2;
/// The sizes at the scale of 100%: the radius of a dot, and the radii a ring goes between.
const DOT_RADIUS: f32 = 14.0;
const RING_FROM: f32 = 10.0;
const RING_TO: f32 = 32.0;

/// What to draw on a picture, in its pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    /// How strong it is, from 0 to 1.
    pub strength: f32,
    /// A ring rather than a dot.
    pub ring: bool,
}

struct Ring {
    x: f32,
    y: f32,
    pressed: f32,
}

struct Dot {
    x: f32,
    y: f32,
    pressed: f32,
    released: Option<f32>,
}

/// The clicks going on in a recording, by what the mouse does from frame to frame.
#[derive(Default)]
pub struct Clicks {
    rings: Vec<Ring>,
    dot: Option<Dot>,
    down: bool,
}

impl Clicks {
    /// What the mouse does at `time`, in seconds of the recording: where it is, whether a button
    /// has been pressed since the last look, which a click shorter than a frame needs, and whether
    /// one is down now.
    pub fn update(&mut self, time: f32, (x, y): (f32, f32), pressed: bool, down: bool) {
        if pressed || (down && !self.down) {
            self.rings.push(Ring { x, y, pressed: time });
            self.dot = Some(Dot { x, y, pressed: time, released: None });
        }
        if let Some(dot) = &mut self.dot {
            if down {
                (dot.x, dot.y) = (x, y);
            } else if dot.released.is_none() {
                dot.released = Some(time);
            }
        }
        self.down = down;
        self.rings.retain(|ring| time - ring.pressed < RING_TIME);
        if self.dot.as_ref().is_some_and(|dot| dot.released.is_some_and(|released| time - released >= DOT_GOES)) {
            self.dot = None;
        }
    }

    /// The marks to draw at `time`, the dot first; `scale` is that of the screen in the picture.
    pub fn marks(&self, time: f32, scale: f32) -> Vec<Mark> {
        let mut marks = Vec::with_capacity(self.rings.len() + 1);
        if let Some(dot) = &self.dot {
            let came = ((time - dot.pressed) / DOT_COMES).clamp(0.0, 1.0);
            let left = dot.released.map_or(1.0, |released| 1.0 - ((time - released) / DOT_GOES).clamp(0.0, 1.0));
            // A click shorter than a frame still shows for a moment.
            let strength = came.max(0.5) * left;
            marks.push(Mark { x: dot.x, y: dot.y, radius: DOT_RADIUS * scale * (0.7 + 0.3 * came), strength, ring: false });
        }
        for ring in &self.rings {
            let gone = ((time - ring.pressed) / RING_TIME).clamp(0.0, 1.0);
            // It goes out fast and slows down, and fades as it goes.
            let out = 1.0 - (1.0 - gone).powi(3);
            let radius = (RING_FROM + (RING_TO - RING_FROM) * out) * scale;
            marks.push(Mark { x: ring.x, y: ring.y, radius, strength: (1.0 - gone).powi(2), ring: true });
        }
        marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: (f32, f32) = (100.0, 50.0);

    #[test]
    fn a_ring_goes_out_and_fades() {
        let mut clicks = Clicks::default();
        clicks.update(1.0, AT, false, true);
        let ring = |clicks: &Clicks, time| *clicks.marks(time, 1.0).iter().find(|mark| mark.ring).unwrap();
        let (start, later) = (ring(&clicks, 1.0), ring(&clicks, 1.2));
        assert_eq!(start.radius, RING_FROM);
        assert_eq!(start.strength, 1.0);
        assert!(later.radius > start.radius && later.strength < start.strength);
        // It is gone when its time is out, though the button is still down.
        clicks.update(1.0 + RING_TIME, AT, false, true);
        assert!(clicks.marks(1.0 + RING_TIME, 1.0).iter().all(|mark| !mark.ring));
    }

    #[test]
    fn a_dot_follows_a_held_button_and_goes_after_it() {
        let mut clicks = Clicks::default();
        clicks.update(1.0, AT, false, true);
        clicks.update(1.1, (140.0, 70.0), false, true);
        let dot = clicks.marks(1.1, 1.0)[0];
        assert_eq!((dot.ring, dot.x, dot.y, dot.strength), (false, 140.0, 70.0, 1.0));
        // Holding on makes no second ring.
        assert_eq!(clicks.rings.len(), 1);
        clicks.update(1.2, (140.0, 70.0), false, false);
        assert_eq!(clicks.marks(1.2, 1.0)[0].strength, 1.0);
        assert!(clicks.marks(1.3, 1.0)[0].strength < 1.0);
        clicks.update(1.2 + DOT_GOES, (140.0, 70.0), false, false);
        assert!(clicks.dot.is_none());
    }

    #[test]
    fn a_click_between_two_frames_is_marked_too() {
        let mut clicks = Clicks::default();
        clicks.update(1.0, AT, true, false);
        let marks = clicks.marks(1.0, 1.0);
        assert!(marks.iter().any(|mark| mark.ring));
        assert!(marks.iter().any(|mark| !mark.ring && mark.strength > 0.0));
    }

    #[test]
    fn a_larger_screen_has_larger_marks() {
        let mut clicks = Clicks::default();
        clicks.update(1.0, AT, false, true);
        let ring = |scale| clicks.marks(1.0, scale).iter().find(|mark| mark.ring).unwrap().radius;
        assert_eq!(ring(2.0), ring(1.0) * 2.0);
    }
}
