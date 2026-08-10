use std::ops::{Add, Mul, Sub};

/// Linear interpolation; `t` outside `[0, 1]` extrapolates.
pub fn lerp<T>(from: T, to: T, t: T) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<Output = T>,
{
    from + (to - from) * t
}
