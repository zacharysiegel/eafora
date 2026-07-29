use crate::map::projection::ProjectedPoint;

/// The camera window in Miller-projected space. Stored projected, not geographic, so pan/zoom
/// arithmetic is uniform on screen: a constant projected increment moves the view a constant screen
/// distance, which a constant latitude increment would not (Miller's `y` is nonlinear in latitude).
/// `x` may fall outside ±π after a horizontal pan past the antimeridian.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub min: ProjectedPoint,
    pub max: ProjectedPoint,
}

impl Viewport {
    /// A viewport with the surface's aspect that fills the surface vertically with the `min_y..max_y`
    /// extent — that extent spans the surface height exactly — and is horizontally centered on
    /// `center_x`, widened by the surface aspect (isotropic, never stretched). On a surface narrower
    /// than the resulting width the horizontal sides fall outside the viewport and are reached by
    /// panning. Assumes `surface` dimensions are nonzero and `max_y > min_y`.
    pub fn fill_height(center_x: f64, min_y: f64, max_y: f64, surface: SurfaceDimensions) -> Viewport {
        let half_height: f64 = (max_y - min_y) / 2.0;
        let center_y: f64 = (min_y + max_y) / 2.0;
        let half_width: f64 = half_height * (surface.width as f64 / surface.height as f64);

        Viewport {
            min: ProjectedPoint { x: center_x - half_width, y: center_y - half_height },
            max: ProjectedPoint { x: center_x + half_width, y: center_y + half_height },
        }
    }
}

/// Physical device pixels: the platform shell multiplies the CSS-pixel cursor position by
/// `devicePixelRatio` so the point shares the device-pixel space of the render surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePoint {
    pub x: f64,
    pub y: f64,
}

/// The attached surface's extent in physical device pixels, the same space as `SurfacePoint`, so a
/// hit-test normalizes the cursor against it before mapping through the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceDimensions {
    pub width: u32,
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER_X: f64 = 0.5;
    const MIN_Y: f64 = -1.25;
    const MAX_Y: f64 = 0.75; // extent midpoint -0.25, half-height 1.0
    const TOLERANCE: f64 = 1e-12;

    fn assert_fill_height_invariants(surface: SurfaceDimensions) {
        let viewport: Viewport = Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface);
        let half_width: f64 = (viewport.max.x - viewport.min.x) / 2.0;
        let half_height: f64 = (viewport.max.y - viewport.min.y) / 2.0;
        let surface_aspect: f64 = surface.width as f64 / surface.height as f64;

        assert!((viewport.min.y - MIN_Y).abs() < TOLERANCE, "min_y fills the surface bottom");
        assert!((viewport.max.y - MAX_Y).abs() < TOLERANCE, "max_y fills the surface top");
        assert!((half_width / half_height - surface_aspect).abs() < TOLERANCE, "aspect matches surface");
        assert!(((viewport.min.x + viewport.max.x) / 2.0 - CENTER_X).abs() < TOLERANCE, "centered x");
    }

    #[test]
    fn fill_height_fills_vertically_and_matches_aspect_on_a_square_surface() {
        assert_fill_height_invariants(SurfaceDimensions { width: 100, height: 100 });
    }

    #[test]
    fn fill_height_widens_the_viewport_on_a_wide_surface() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 400, height: 100 };
        assert_fill_height_invariants(surface);

        let viewport: Viewport = Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface);
        assert!((viewport.max.x - viewport.min.x) / 2.0 > (MAX_Y - MIN_Y) / 2.0, "wider than tall");
    }

    #[test]
    fn fill_height_narrows_the_viewport_on_a_tall_surface() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 400 };
        assert_fill_height_invariants(surface);

        let viewport: Viewport = Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface);
        assert!((viewport.max.x - viewport.min.x) / 2.0 < (MAX_Y - MIN_Y) / 2.0, "narrower than tall");
    }
}
