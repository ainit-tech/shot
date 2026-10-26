//! The magic pencil (decision 6): a rough loop drawn around something becomes the neat ellipse
//! or rectangle that fits it best. Only the points of the loop are used, no AI.

use crate::annotation::Shape;
use crate::geometry::PointF;

/// How far apart the ends of a loop may be, as a part of its size.
const MAX_GAP: f32 = 0.35;
/// How near the end of a stroke must pass by its start to have closed the loop, as a part of its
/// size.
const REACHED: f32 = 0.1;
/// How badly a neat shape may fit the loop, as a part of its size, and still replace it.
const MAX_ERROR: f32 = 0.2;
/// How many points spread evenly along the loop are compared with a shape.
const SAMPLES: usize = 96;

/// The neat shape for a loop, or nothing if the stroke is not a loop or no shape fits it.
pub fn fit(points: &[PointF]) -> Option<Shape> {
    if points.len() < 8 {
        return None;
    }
    let size = extent(points);
    let points = closed(points);
    if size < 8.0 || points[0].distance(*points.last()?) > MAX_GAP * size {
        return None;
    }
    let samples = resample(points, SAMPLES)?;
    [fit_ellipse(&samples), fit_rectangle(&samples)]
        .into_iter()
        .flatten()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, error)| *error <= MAX_ERROR)
        .map(|(shape, _)| shape)
}

/// The loop without what was drawn past its start. A hand closing a loop often goes on over its
/// beginning, and that part, drawn twice, would weigh twice, pulling the ellipse off the loop: the
/// loop ends where the second half of the stroke comes nearest to the start, if it comes near
/// enough to have reached it, rather than across a long loop that stops short.
fn closed(points: &[PointF]) -> &[PointF] {
    let (start, size) = (points[0], extent(points));
    let nearest = (points.len() / 2..points.len()).min_by(|&a, &b| points[a].distance(start).total_cmp(&points[b].distance(start)));
    match nearest {
        Some(end) if points[end].distance(start) <= REACHED * size => &points[..=end],
        _ => points,
    }
}

/// The diagonal of the box around the points.
fn extent(points: &[PointF]) -> f32 {
    let (mut min, mut max) = (points[0], points[0]);
    for p in points {
        min = PointF::new(min.x.min(p.x), min.y.min(p.y));
        max = PointF::new(max.x.max(p.x), max.y.max(p.y));
    }
    min.distance(max)
}

/// Points spread evenly along the closed loop, so that the parts drawn slowly don't weigh more.
fn resample(points: &[PointF], count: usize) -> Option<Vec<PointF>> {
    let mut path = points.to_vec();
    path.push(points[0]);
    let lengths: Vec<f32> = path.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f32 = lengths.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let mut samples = Vec::with_capacity(count);
    let (mut segment, mut walked) = (0, 0.0);
    for i in 0..count {
        let target = total * i as f32 / count as f32;
        while segment + 1 < lengths.len() && walked + lengths[segment] < target {
            walked += lengths[segment];
            segment += 1;
        }
        let t = if lengths[segment] > 0.0 { ((target - walked) / lengths[segment]).clamp(0.0, 1.0) } else { 0.0 };
        let (a, b) = (path[segment], path[segment + 1]);
        samples.push(PointF::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
    }
    Some(samples)
}

/// The ellipse along the main directions of the points' spread, and how badly it fits.
fn fit_ellipse(samples: &[PointF]) -> Option<(Shape, f32)> {
    let n = samples.len() as f32;
    let center = PointF::new(
        samples.iter().map(|p| p.x).sum::<f32>() / n,
        samples.iter().map(|p| p.y).sum::<f32>() / n,
    );
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for p in samples {
        let (dx, dy) = (p.x - center.x, p.y - center.y);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (cos, sin) = (angle.cos(), angle.sin());
    let along = |p: &PointF| {
        let (dx, dy) = (p.x - center.x, p.y - center.y);
        (dx * cos + dy * sin, dy * cos - dx * sin)
    };
    let (mut su, mut sv) = (0.0, 0.0);
    for p in samples {
        let (u, v) = along(p);
        su += u * u;
        sv += v * v;
    }
    // Points spread evenly around a circle have a mean square of half the radius squared along
    // each axis; around a long ellipse they crowd on its long sides, which would make it rounder
    // than drawn. The radii are that first guess, put right by the least squares of
    // (u/a)² + (v/b)² = 1, which holds wherever the points are.
    let guess = ((2.0 * su / n).sqrt(), (2.0 * sv / n).sqrt());
    if guess.0 < 2.0 || guess.1 < 2.0 {
        return None;
    }
    let (mut uuuu, mut uuvv, mut vvvv, mut uu, mut vv) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for p in samples {
        let (u, v) = along(p);
        let (u, v) = ((u / guess.0).powi(2), (v / guess.1).powi(2));
        (uuuu, uuvv, vvvv, uu, vv) = (uuuu + u * u, uuvv + u * v, vvvv + v * v, uu + u, vv + v);
    }
    let det = uuuu * vvvv - uuvv * uuvv;
    let (a, b) = ((uu * vvvv - vv * uuvv) / det, (vv * uuuu - uu * uuvv) / det);
    let radii = if a > 0.0 && b > 0.0 { (guess.0 / a.sqrt(), guess.1 / b.sqrt()) } else { guess };
    let error = samples
        .iter()
        .map(|p| {
            let (u, v) = along(p);
            ((u / radii.0).hypot(v / radii.1) - 1.0).abs()
        })
        .sum::<f32>()
        / n;
    Some((Shape::Ellipse { center, radii, angle }, error))
}

/// The rectangle with sides along the screen's edges, as things on a screen are, and how badly
/// it fits. The few outermost points are left out, since the ends of a loop often overshoot.
fn fit_rectangle(samples: &[PointF]) -> Option<(Shape, f32)> {
    let sorted = |value: fn(&PointF) -> f32| {
        let mut values: Vec<f32> = samples.iter().map(value).collect();
        values.sort_by(f32::total_cmp);
        values
    };
    let (xs, ys) = (sorted(|p| p.x), sorted(|p| p.y));
    let trim = samples.len() / 40;
    let (left, right) = (xs[trim], xs[xs.len() - 1 - trim]);
    let (top, bottom) = (ys[trim], ys[ys.len() - 1 - trim]);
    let (width, height) = (right - left, bottom - top);
    if width < 2.0 || height < 2.0 {
        return None;
    }
    let distance = |p: &PointF| {
        let outside_x = (left - p.x).max(p.x - right).max(0.0);
        let outside_y = (top - p.y).max(p.y - bottom).max(0.0);
        if outside_x > 0.0 || outside_y > 0.0 {
            outside_x.hypot(outside_y)
        } else {
            (p.x - left).min(right - p.x).min(p.y - top).min(bottom - p.y)
        }
    };
    let n = samples.len() as f32;
    let error = samples.iter().map(distance).sum::<f32>() / n / ((width + height) / 4.0);
    Some((Shape::Rectangle(PointF::new(left, top), PointF::new(right, bottom)), error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// A loop drawn by a not quite steady hand: the radius wobbles and the ends don't meet.
    fn rough_circle() -> Vec<PointF> {
        (0..60)
            .map(|i| {
                let a = TAU * i as f32 / 64.0;
                let r = 50.0 + 2.0 * (7.0 * a).sin();
                PointF::new(200.0 + r * a.cos(), 100.0 + r * a.sin())
            })
            .collect()
    }

    fn rough_box() -> Vec<PointF> {
        let corners = [(0.0, 0.0), (120.0, 2.0), (118.0, 80.0), (-2.0, 78.0), (3.0, 4.0)];
        let mut points = Vec::new();
        for w in corners.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            for i in 0..20 {
                let t = i as f32 / 20.0;
                points.push(PointF::new(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t));
            }
        }
        points
    }

    #[test]
    fn a_rough_circle_becomes_an_ellipse() {
        let Some(Shape::Ellipse { center, radii, .. }) = fit(&rough_circle()) else { panic!("not an ellipse") };
        assert!(center.distance(PointF::new(200.0, 100.0)) < 5.0);
        assert!((radii.0 - 50.0).abs() < 6.0 && (radii.1 - 50.0).abs() < 6.0);
    }

    #[test]
    fn a_rough_box_becomes_a_rectangle() {
        let Some(Shape::Rectangle(a, b)) = fit(&rough_box()) else { panic!("not a rectangle") };
        assert!(a.distance(PointF::new(0.0, 0.0)) < 6.0);
        assert!(b.distance(PointF::new(120.0, 80.0)) < 6.0);
    }

    /// Numbers for the survey below, the same on every run.
    struct Dice(u64);

    impl Dice {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32
        }

        fn range(&mut self, low: f32, high: f32) -> f32 {
            low + (high - low) * self.next()
        }
    }

    /// How a hand draws a loop: how much its size wavers along the way, how much it shakes in
    /// pixels, how far past the start it ends (less than nothing: short of it), how far it tilts.
    struct Hand {
        wobble: f32,
        shake: f32,
        closure: (f32, f32),
        tilt: f32,
    }

    const CAREFUL: Hand = Hand { wobble: 0.02, shake: 0.5, closure: (-0.05, 0.05), tilt: 0.03 };
    const USUAL: Hand = Hand { wobble: 0.04, shake: 1.0, closure: (-0.15, 0.1), tilt: 0.05 };
    const HASTY: Hand = Hand { wobble: 0.07, shake: 2.0, closure: (-0.25, 0.15), tilt: 0.08 };

    /// A loop drawn by a hand around an outline, which gives the point a part of the way round,
    /// between -1 and 1 on each axis; `half` is the outline's half size in pixels.
    fn drawn(outline: &dyn Fn(f32) -> (f32, f32), half: (f32, f32), tilt: f32, hand: &Hand, dice: &mut Dice) -> Vec<PointF> {
        let (start, count) = (dice.next(), 90 + (dice.next() * 60.0) as usize);
        let (phase, waves) = (dice.range(0.0, TAU), if dice.next() < 0.5 { 2.0 } else { 3.0 });
        let tilt = tilt + dice.range(-hand.tilt, hand.tilt);
        let (cos, sin) = (tilt.cos(), tilt.sin());
        let sweep = 1.0 + dice.range(hand.closure.0, hand.closure.1);
        let (mut t, mut points) = (0.0, Vec::new());
        while t <= sweep {
            let (x, y) = outline(start + t);
            let scale = 1.0 + hand.wobble * (waves * TAU * t + phase).sin();
            let (x, y) = (x * half.0 * scale, y * half.1 * scale);
            let shake = (dice.range(-hand.shake, hand.shake), dice.range(-hand.shake, hand.shake));
            points.push(PointF::new(300.0 + x * cos - y * sin + shake.0, 300.0 + x * sin + y * cos + shake.1));
            // The hand moves unevenly.
            t += sweep / count as f32 * dice.range(0.4, 1.6);
        }
        points
    }

    /// The outline between an ellipse, 2, and a box, the larger the more.
    fn superellipse(power: f32) -> impl Fn(f32) -> (f32, f32) {
        move |t| {
            let (c, s) = ((TAU * t).cos(), (TAU * t).sin());
            (c.signum() * c.abs().powf(2.0 / power), s.signum() * s.abs().powf(2.0 / power))
        }
    }

    /// A box with sharp corners.
    fn sharp_box(t: f32) -> (f32, f32) {
        let u = t.rem_euclid(1.0) * 4.0;
        let f = 2.0 * u.fract() - 1.0;
        match u as i32 {
            0 => (f, -1.0),
            1 => (1.0, f),
            2 => (-f, 1.0),
            _ => (-1.0, -f),
        }
    }

    /// What the magic pencil makes of loops drawn by hand: circles and ovals of several
    /// proportions, boxes, and the shapes between them, each drawn 300 times by a careful, a usual
    /// and a hasty hand. It prints the share of ellipses, rectangles and loops left as drawn; it
    /// runs only when asked: `cargo test -p ainitshot magic_survey -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn magic_survey() {
        type Outline = Box<dyn Fn(f32) -> (f32, f32)>;
        let shapes: Vec<(&str, Outline, f32, f32)> = vec![
            ("circle", Box::new(superellipse(2.0)), 1.0, 0.0),
            ("oval 1.5:1", Box::new(superellipse(2.0)), 1.5, 0.0),
            ("oval 2:1", Box::new(superellipse(2.0)), 2.0, 0.0),
            ("oval 3:1", Box::new(superellipse(2.0)), 3.0, 0.0),
            ("oval 2:1 tilted 30°", Box::new(superellipse(2.0)), 2.0, 0.52),
            ("between, power 2.5, 1.5:1", Box::new(superellipse(2.5)), 1.5, 0.0),
            ("between, power 3, 1.5:1", Box::new(superellipse(3.0)), 1.5, 0.0),
            ("between, power 4, 1.5:1", Box::new(superellipse(4.0)), 1.5, 0.0),
            ("rounded box, power 6, 1.5:1", Box::new(superellipse(6.0)), 1.5, 0.0),
            ("box 1:1", Box::new(sharp_box), 1.0, 0.0),
            ("box 1.5:1", Box::new(sharp_box), 1.5, 0.0),
            ("box 3:1", Box::new(sharp_box), 3.0, 0.0),
        ];
        let mut dice = Dice(0x9E37_79B9_7F4A_7C15);
        println!("{:<30} {:>24} {:>24} {:>24}", "drawn", "careful", "usual", "hasty");
        for (name, outline, aspect, tilt) in &shapes {
            let mut row = format!("{name:<30}");
            for hand in [&CAREFUL, &USUAL, &HASTY] {
                let (mut ellipses, mut rectangles, mut neither) = (0, 0, 0);
                for _ in 0..300 {
                    let height = dice.range(40.0, 150.0);
                    match fit(&drawn(outline.as_ref(), (height * aspect, height), *tilt, hand, &mut dice)) {
                        Some(Shape::Ellipse { .. }) => ellipses += 1,
                        Some(_) => rectangles += 1,
                        None => neither += 1,
                    }
                }
                row += &format!(" {:>7}{:>8}{:>9}", format!("o{}", ellipses * 100 / 300), format!("r{}", rectangles * 100 / 300), format!("-{}", neither * 100 / 300));
            }
            println!("{row}");
        }
    }

    /// An oval three times as long as it is high, drawn steadily from a little past its right
    /// end, `sweep` times round.
    fn long_oval(sweep: f32) -> Vec<PointF> {
        (0..=(120.0 * sweep) as usize)
            .map(|i| {
                let a = TAU * i as f32 / 120.0 + 0.3;
                PointF::new(300.0 + 150.0 * a.cos(), 200.0 + 50.0 * a.sin())
            })
            .collect()
    }

    #[test]
    fn a_long_oval_keeps_its_proportions() {
        // The points of a long oval crowd on its long sides, which once made the ellipse rounder
        // than drawn, and a rectangle won.
        let Some(Shape::Ellipse { center, radii, .. }) = fit(&long_oval(1.0)) else { panic!("not an ellipse") };
        assert!(center.distance(PointF::new(300.0, 200.0)) < 2.0, "{center:?}");
        assert!((radii.0 - 150.0).abs() < 3.0 && (radii.1 - 50.0).abs() < 3.0, "{radii:?}");
    }

    #[test]
    fn a_loop_drawn_past_its_start_stays_an_oval() {
        // The hand goes on a tenth of the way round over where it began.
        assert!(matches!(fit(&long_oval(1.1)), Some(Shape::Ellipse { .. })), "{:?}", fit(&long_oval(1.1)));
        let circle: Vec<PointF> =
            (0..=66).map(|i| PointF::new(200.0 + 50.0 * (TAU * i as f32 / 60.0).cos(), 100.0 + 50.0 * (TAU * i as f32 / 60.0).sin())).collect();
        assert!(matches!(fit(&circle), Some(Shape::Ellipse { .. })), "{:?}", fit(&circle));
    }

    #[test]
    fn a_stroke_that_is_not_a_loop_stays_as_drawn() {
        let zigzag: Vec<PointF> = (0..40).map(|i| PointF::new(i as f32 * 5.0, if i % 2 == 0 { 0.0 } else { 15.0 })).collect();
        assert_eq!(fit(&zigzag), None);
    }
}
