//! Smallest rotated rectangle around a 2D point set.

pub type Point = [f64; 2];

/// A rectangle in the plane. `length` lies along `angle_rad` and is never
/// shorter than `width`; `angle_rad` is normalized to (-π/2, π/2].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrientedRect {
    pub center: Point,
    pub length: f64,
    pub width: f64,
    pub angle_rad: f64,
}

/// Minimum-area enclosing rectangle. One side of the optimum is always
/// collinear with a convex-hull edge, so only those orientations are tried.
#[must_use]
pub fn min_area_rect(points: &[Point]) -> Option<OrientedRect> {
    let hull = convex_hull(points);
    match hull.len() {
        0 => return None,
        1 => {
            return Some(OrientedRect {
                center: hull[0],
                length: 0.0,
                width: 0.0,
                angle_rad: 0.0,
            });
        }
        _ => {}
    }

    let mut best: Option<(f64, OrientedRect)> = None;
    for index in 0..hull.len() {
        let start = hull[index];
        let end = hull[(index + 1) % hull.len()];
        let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
        let edge_length = dx.hypot(dy);
        if edge_length <= f64::EPSILON {
            continue;
        }
        let axis = [dx / edge_length, dy / edge_length];
        let normal = [-axis[1], axis[0]];

        let (mut min_u, mut max_u, mut min_v, mut max_v) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for point in &hull {
            let u = point[0] * axis[0] + point[1] * axis[1];
            let v = point[0] * normal[0] + point[1] * normal[1];
            min_u = min_u.min(u);
            max_u = max_u.max(u);
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }

        let area = (max_u - min_u) * (max_v - min_v);
        if best
            .as_ref()
            .is_some_and(|(best_area, _)| *best_area <= area)
        {
            continue;
        }
        let (mid_u, mid_v) = ((min_u + max_u) / 2.0, (min_v + max_v) / 2.0);
        let center = [
            mid_u * axis[0] + mid_v * normal[0],
            mid_u * axis[1] + mid_v * normal[1],
        ];
        let rect = OrientedRect {
            center,
            length: max_u - min_u,
            width: max_v - min_v,
            angle_rad: axis[1].atan2(axis[0]),
        };
        best = Some((area, normalized(rect)));
    }

    best.map(|(_, rect)| rect)
}

/// Puts the long side along the angle and folds the angle into (-π/2, π/2].
fn normalized(mut rect: OrientedRect) -> OrientedRect {
    use std::f64::consts::{FRAC_PI_2, PI};

    if rect.width > rect.length {
        std::mem::swap(&mut rect.width, &mut rect.length);
        rect.angle_rad += FRAC_PI_2;
    }
    while rect.angle_rad > FRAC_PI_2 {
        rect.angle_rad -= PI;
    }
    while rect.angle_rad <= -FRAC_PI_2 {
        rect.angle_rad += PI;
    }
    rect
}

/// Andrew's monotone chain; returns the hull counter-clockwise without
/// collinear points.
fn convex_hull(points: &[Point]) -> Vec<Point> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }

    let cross = |o: Point, a: Point, b: Point| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<Point> = Vec::with_capacity(sorted.len() * 2);
    for pass in [false, true] {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &Point>> = if pass {
            Box::new(sorted.iter().rev())
        } else {
            Box::new(sorted.iter())
        };
        for &point in ordered {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0
            {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rotated_rectangle(center: Point, length: f64, width: f64, angle: f64) -> Vec<Point> {
        let (sin, cos) = angle.sin_cos();
        let mut points = Vec::new();
        for i in 0..=20 {
            for j in 0..=10 {
                let u = (f64::from(i) / 20.0 - 0.5) * length;
                let v = (f64::from(j) / 10.0 - 0.5) * width;
                points.push([center[0] + u * cos - v * sin, center[1] + u * sin + v * cos]);
            }
        }
        points
    }

    #[test]
    fn recovers_a_rotated_rectangle() {
        let angle = 30_f64.to_radians();
        let rect = min_area_rect(&rotated_rectangle([2.0, -1.0], 2.0, 0.8, angle)).unwrap();
        assert!((rect.length - 2.0).abs() < 1e-9, "{rect:?}");
        assert!((rect.width - 0.8).abs() < 1e-9, "{rect:?}");
        assert!((rect.angle_rad - angle).abs() < 1e-9, "{rect:?}");
        assert!((rect.center[0] - 2.0).abs() < 1e-9 && (rect.center[1] + 1.0).abs() < 1e-9);
    }

    #[test]
    fn puts_the_long_side_along_the_angle() {
        let rect = min_area_rect(&rotated_rectangle([0.0, 0.0], 0.5, 1.5, 0.0)).unwrap();
        assert!((rect.length - 1.5).abs() < 1e-9 && (rect.width - 0.5).abs() < 1e-9);
        assert!((rect.angle_rad.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }
}
