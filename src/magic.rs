//! The magic pencil (decision 6): a rough loop drawn around something becomes the neat ellipse
//! or rectangle that fits it best. Only the points of the loop are used, no AI.

use crate::annotation::Shape;
use crate::geometry::PointF;

/// How far apart the ends of a loop may be, as a part of its size.
const MAX_GAP: f32 = 0.35;
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
    // Points spread evenly around an ellipse have a mean square of half the radius squared
    // along each of its axes.
    let radii = ((2.0 * su / n).sqrt(), (2.0 * sv / n).sqrt());
    if radii.0 < 2.0 || radii.1 < 2.0 {
        return None;
    }
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

    #[test]
    fn a_stroke_that_is_not_a_loop_stays_as_drawn() {
        let zigzag: Vec<PointF> = (0..40).map(|i| PointF::new(i as f32 * 5.0, if i % 2 == 0 { 0.0 } else { 15.0 })).collect();
        assert_eq!(fit(&zigzag), None);
    }
}
