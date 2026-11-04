//! Smooth lines through measured points that can't show what wasn't measured.

/// Slope at each point of `points`, as (x, y) with x increasing, by the
/// Fritsch-Carlson method. Unlike a plain spline, the line never rises above or
/// dips below the points either side of it.
pub fn slopes(points: &[(f32, f32)]) -> Vec<f32> {
    let secant = |pair: &[(f32, f32)]| (pair[1].1 - pair[0].1) / (pair[1].0 - pair[0].0);
    let mut slopes = vec![0.0; points.len()];
    if points.len() < 2 {
        return slopes;
    }
    slopes[0] = secant(&points[..2]);
    slopes[points.len() - 1] = secant(&points[points.len() - 2..]);
    for index in 1..points.len() - 1 {
        let (before, after) = (
            secant(&points[index - 1..=index]),
            secant(&points[index..=index + 1]),
        );
        if before * after > 0.0 {
            // A harmonic mean, weighted towards the nearer neighbour.
            let (left, right) = (
                points[index].0 - points[index - 1].0,
                points[index + 1].0 - points[index].0,
            );
            let (first, second) = (2.0 * right + left, right + 2.0 * left);
            slopes[index] = (first + second) / (first / before + second / after);
        }
    }
    slopes
}

/// Control points of the Bézier segment between two points and their slopes.
pub fn controls(
    from: (f32, f32),
    to: (f32, f32),
    from_slope: f32,
    to_slope: f32,
) -> ((f32, f32), (f32, f32)) {
    let third = (to.0 - from.0) / 3.0;
    (
        (from.0 + third, from.1 + from_slope * third),
        (to.0 - third, to.1 - to_slope * third),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point `t` of the way along the Bézier segment after `index`.
    fn sample(points: &[(f32, f32)], slopes: &[f32], index: usize, t: f32) -> f32 {
        let (from, to) = (points[index], points[index + 1]);
        let (first, second) = controls(from, to, slopes[index], slopes[index + 1]);
        let u = 1.0 - t;
        u * u * u * from.1
            + 3.0 * u * u * t * first.1
            + 3.0 * u * t * t * second.1
            + t * t * t * to.1
    }

    #[test]
    fn the_curve_never_shows_a_level_beyond_the_points_either_side() {
        let curves: [&[(f32, f32)]; 4] = [
            // A discharge with an uneven sampling interval.
            &[
                (0.0, 90.0),
                (1.0, 89.0),
                (30.0, 60.0),
                (31.0, 59.0),
                (90.0, 20.0),
            ],
            // Charging straight after a flat stretch.
            &[
                (0.0, 40.0),
                (10.0, 40.0),
                (11.0, 41.0),
                (12.0, 80.0),
                (60.0, 100.0),
            ],
            // The worst case for a spline: a sharp reversal.
            &[
                (0.0, 0.0),
                (1.0, 100.0),
                (2.0, 0.0),
                (50.0, 100.0),
                (51.0, 0.0),
            ],
            &[(0.0, 50.0), (5.0, 50.0), (6.0, 50.0)],
        ];
        for points in curves {
            let slopes = slopes(points);
            for index in 0..points.len() - 1 {
                let low = points[index].1.min(points[index + 1].1) - 1e-2;
                let high = points[index].1.max(points[index + 1].1) + 1e-2;
                for step in 0..=20_u8 {
                    let value = sample(points, &slopes, index, f32::from(step) / 20.0);
                    assert!(
                        (low..=high).contains(&value),
                        "{value} after point {index} of {points:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_flat_line_stays_flat() {
        assert_eq!(slopes(&[(0.0, 3.0), (1.0, 3.0), (9.0, 3.0)]), [0.0; 3]);
    }

    #[test]
    fn too_few_points_for_a_slope_are_flat() {
        assert_eq!(slopes(&[]), Vec::<f32>::new());
        assert_eq!(slopes(&[(4.0, 1.0)]), [0.0]);
    }
}
