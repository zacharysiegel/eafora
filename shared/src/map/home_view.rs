use crate::map::projection::{self, GeoPoint, ProjectedPoint};
use crate::map::viewport::{SurfaceDimensions, Viewport};

/// Greenwich, on the prime meridian. Only the longitude is used.
pub const HOME_CENTER: GeoPoint = GeoPoint {
    lat: 51.4779,
    lon: 0.0,
};

/// The home view's latitude framing, in degrees: chosen by hand to enclose the drawn continents (Tierra
/// del Fuego to northern Greenland) with no empty polar ocean.
const HOME_VIEW_MIN_LAT: f64 = -56.0;
const HOME_VIEW_MAX_LAT: f64 = 84.0;

/// The `HOME_VIEW_MIN_LAT`..`HOME_VIEW_MAX_LAT` band fills the surface vertically, centered on the prime
/// meridian.
pub fn home_viewport(surface_dimensions: SurfaceDimensions) -> Viewport {
    let center_x: f64 = projection::project(HOME_VIEW_MIN_LAT, HOME_CENTER.lon).x;
    let (min_y, max_y): (f64, f64) = home_range_projected_y_bounds();

    Viewport::fill_height(center_x, min_y, max_y, surface_dimensions)
}

/// The home latitude range's lower and upper bounds in projected space, the vertical limits pan and
/// zoom-out clamp against.
pub fn home_range_projected_y_bounds() -> (f64, f64) {
    let southern_edge: ProjectedPoint = projection::project(HOME_VIEW_MIN_LAT, HOME_CENTER.lon);
    let northern_edge: ProjectedPoint = projection::project(HOME_VIEW_MAX_LAT, HOME_CENTER.lon);

    (southern_edge.y, northern_edge.y)
}

/// The largest height (furthest zoom-out): the home range, capped so the aspect-locked width never
/// exceeds one world turn.
pub fn zoom_out_ceiling_height(surface_dimensions: SurfaceDimensions) -> f64 {
    let (min_y, max_y): (f64, f64) = home_range_projected_y_bounds();
    let home_height: f64 = max_y - min_y;
    let width_cap_height: f64 = std::f64::consts::TAU * (surface_dimensions.height as f64 / surface_dimensions.width as f64);

    home_height.min(width_cap_height)
}

/// Keeps the current pan and zoom across a change of surface size, re-fitting only the aspect.
pub fn refit_to_surface(viewport: Viewport, surface_dimensions: SurfaceDimensions) -> Viewport {
    let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
    let ceiling: f64 = zoom_out_ceiling_height(surface_dimensions);

    viewport.refit_to_surface(surface_dimensions, ceiling, home_min_y, home_max_y)
}
