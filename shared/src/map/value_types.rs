use chrono::NaiveDate;

use crate::canonical::StatisticKind;
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

/// A region's `code` slug (e.g. `"usa"`, `"germany"`), wrapping the canonical `region.code`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionCode(pub String);

/// The per-frame inputs the renderer needs beyond the `Viewport`.
#[derive(Debug, Clone)]
pub struct FrameState {
    pub active_statistic: StatisticKind,
    /// The year scrubber's current position; a single date, not a range, hence `_start` (a period
    /// is a [start, end] pair).
    pub active_period_start: NaiveDate,
    pub selected_region: Option<RegionCode>,
    pub hovered_region: Option<RegionCode>,
}

/// Platform window/layer pointers marshaled from the native shell. No `Wasm` variant: the web path
/// attaches its surface from an `HtmlCanvasElement` directly rather than through a window handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowHandle {
    UiKit { layer_ptr: u64, view_ptr: u64 },
    AndroidNdk { native_window_ptr: u64 },
}
