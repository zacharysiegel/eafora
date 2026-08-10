use std::f64::consts::TAU;
use std::ops::{Add, Mul, Sub};

/// Linear interpolation; `t` outside `[0, 1]` extrapolates.
pub fn lerp<T>(from: T, to: T, t: T) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<Output = T>,
{
    from + (to - from) * t
}

/// The parameter at which `lerp(from, to, t)` equals `value`; the inverse of `lerp`.
pub fn inverse_lerp(from: f64, to: f64, value: f64) -> f64 {
    (value - from) / (to - from)
}

/// Geometric interpolation: a blend linear in the logarithm, so the ratio between successive `t` steps is
/// constant (the `t = 0.5` value is the geometric mean of the ends). `from` and `to` must be strictly
/// positive.
pub fn geometric_interpolate(from: f64, to: f64, t: f64) -> f64 {
    from * (to / from).powf(t)
}

/// `target` shifted by whole turns of `2π` to land within ±π of `reference`: the representation of the
/// same angle reachable by the shortest move.
pub fn unwrap_nearest(reference: f64, target: f64) -> f64 {
    target - ((target - reference) / TAU).round() * TAU
}

/// Cubic ease-in-out; pinned to `p(0) = 0` and `p(1) = 1`.
pub fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let f: f64 = -2.0 * t + 2.0;
        1.0 - f * f * f / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f64 = 1e-9;

    #[test]
    fn lerp_and_inverse_lerp_are_inverses() {
        let interpolated: f64 = lerp(2.0, 8.0, 0.25);
        assert!((interpolated - 3.5).abs() < TOLERANCE);
        assert!((inverse_lerp(2.0, 8.0, 3.5) - 0.25).abs() < TOLERANCE);
    }

    #[test]
    fn geometric_interpolate_endpoints_and_constant_ratio() {
        assert!((geometric_interpolate(2.0, 8.0, 0.0) - 2.0).abs() < TOLERANCE);
        assert!((geometric_interpolate(2.0, 8.0, 1.0) - 8.0).abs() < TOLERANCE);
        // Constant ratio: the midpoint is the geometric mean, so it squares to the product of the ends.
        let mid: f64 = geometric_interpolate(2.0, 8.0, 0.5);
        assert!((mid - 4.0).abs() < TOLERANCE, "geometric midpoint of 2 and 8 is 4");
    }

    #[test]
    fn unwrap_nearest_shifts_to_the_near_representative() {
        // -3 rad is more than half a turn from +3 rad; the near representative is +3.28 (one turn up).
        let unwrapped: f64 = unwrap_nearest(3.0, -3.0);
        assert!((unwrapped - (-3.0 + TAU)).abs() < TOLERANCE);
        assert!((unwrap_nearest(0.5, 0.7) - 0.7).abs() < TOLERANCE, "already within a turn is unchanged");
        let shift: f64 = unwrap_nearest(3.0, -3.0) - (-3.0);
        assert!((shift / TAU - (shift / TAU).round()).abs() < 1e-12, "shift is a whole number of turns");
    }

    #[test]
    fn ease_in_out_cubic_is_pinned_and_symmetric() {
        assert!(ease_in_out_cubic(0.0).abs() < 1e-12);
        assert!((ease_in_out_cubic(1.0) - 1.0).abs() < 1e-12);
        assert!((ease_in_out_cubic(0.5) - 0.5).abs() < 1e-12);

        let mut previous: f64 = -1.0;
        for step in 0..=20 {
            let value: f64 = ease_in_out_cubic(step as f64 / 20.0);
            assert!(value >= previous, "monotonic increasing");
            previous = value;
        }
    }
}
