/// Longitudes may fall outside ±180 after a horizontal pan past the antimeridian.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub longitude_min: f64,
    pub longitude_max: f64,
    pub latitude_min: f64,
    pub latitude_max: f64,
}

/// Device-pixel-logical coordinates; the platform shell pre-divides by `devicePixelRatio` before
/// passing them in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    pub x: f64,
    pub y: f64,
}

/// A region's `code` slug (e.g. `"usa"`, `"germany"`), wrapping the canonical `region.code`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionCode(pub String);
