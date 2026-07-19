use crate::map::projection::ProjectedPoint;

/// The camera window in Miller-projected space. Stored projected, not geographic, so pan/zoom
/// arithmetic is uniform on screen: a constant projected increment moves the view a constant screen
/// distance, which a constant latitude increment would not (Miller's `y` is nonlinear in latitude).
/// `x` may fall outside ±180 after a horizontal pan past the antimeridian.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub min: ProjectedPoint,
    pub max: ProjectedPoint,
}

/// Device-pixel-logical coordinates; the platform shell pre-divides by `devicePixelRatio` before
/// passing them in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    pub x: f64,
    pub y: f64,
}

/// The attached surface's extent, in the same device-pixel-logical space as `ScreenPoint`. A hit-test
/// needs it to normalize a device-pixel point against the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceDimensions {
    pub width: u32,
    pub height: u32,
}
