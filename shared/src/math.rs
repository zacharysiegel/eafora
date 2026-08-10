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

/// A cubic ease-in-out: maps `[0, 1]` monotonically onto `[0, 1]`, pinned to `p(0) = 0` and `p(1) = 1`,
/// accelerating off the start and decelerating onto the end.
pub fn cubic_ease_in_out(t: f64) -> f64 {
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
    fn cubic_ease_in_out_is_pinned_and_symmetric() {
        assert!(cubic_ease_in_out(0.0).abs() < 1e-12);
        assert!((cubic_ease_in_out(1.0) - 1.0).abs() < 1e-12);
        assert!((cubic_ease_in_out(0.5) - 0.5).abs() < 1e-12);

        let mut previous: f64 = -1.0;
        for step in 0..=20 {
            let value: f64 = cubic_ease_in_out(step as f64 / 20.0);
            assert!(value >= previous, "monotonic increasing");
            previous = value;
        }
    }
}
