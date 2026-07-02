//! The map's public value types: the geographic camera bounds (`Viewport`), a screen-space input
//! point (`ScreenPoint`), and a region identifier (`RegionCode`) — the hit-test's inputs and output.

/// The camera's current geographic bounds. Longitudes may fall outside ±180 after a horizontal pan
/// past the antimeridian; the hit-test wraps them back before querying.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub longitude_min: f64,
    pub longitude_max: f64,
    pub latitude_min: f64,
    pub latitude_max: f64,
}

/// A device-pixel-logical screen coordinate. The platform shell pre-divides by `devicePixelRatio`
/// before passing it in, so the hit-test never sees physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    pub x: f64,
    pub y: f64,
}

/// A region's `code` slug (e.g. `"usa"`, `"germany"`), wrapping the canonical `region.code`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionCode(pub String);
