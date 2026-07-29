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
    /// A viewport with the surface's aspect that fills the surface vertically: `center ± half_height` in
    /// y (the full requested vertical extent spans the surface height) and `center ± half_height *
    /// surface_aspect` in x (isotropic, so the projected content is never stretched). The horizontal
    /// extent is whatever the aspect yields; on a surface narrower than the content the sides fall
    /// outside the viewport and are reached by panning. Assumes `surface` dimensions are nonzero.
    pub fn fill_height(center: ProjectedPoint, half_height: f64, surface: SurfaceDimensions) -> Viewport {
        let surface_aspect: f64 = surface.width as f64 / surface.height as f64;
        let half_width: f64 = half_height * surface_aspect;

        Viewport {
            min: ProjectedPoint { x: center.x - half_width, y: center.y - half_height },
            max: ProjectedPoint { x: center.x + half_width, y: center.y + half_height },
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

    const CENTER: ProjectedPoint = ProjectedPoint { x: 0.5, y: -0.25 };
    const HALF_HEIGHT: f64 = 1.0;
    const TOLERANCE: f64 = 1e-12;

    fn assert_fill_height_invariants(surface: SurfaceDimensions) {
        let viewport: Viewport = Viewport::fill_height(CENTER, HALF_HEIGHT, surface);
        let half_width: f64 = (viewport.max.x - viewport.min.x) / 2.0;
        let half_height: f64 = (viewport.max.y - viewport.min.y) / 2.0;

        let surface_aspect: f64 = surface.width as f64 / surface.height as f64;
        assert!((half_height - HALF_HEIGHT).abs() < TOLERANCE, "height pinned");
        assert!((half_width / half_height - surface_aspect).abs() < TOLERANCE, "aspect matches surface");
        assert!(((viewport.min.x + viewport.max.x) / 2.0 - CENTER.x).abs() < TOLERANCE, "centered x");
        assert!(((viewport.min.y + viewport.max.y) / 2.0 - CENTER.y).abs() < TOLERANCE, "centered y");
    }

    #[test]
    fn fill_height_pins_height_and_matches_aspect_on_a_square_surface() {
        assert_fill_height_invariants(SurfaceDimensions { width: 100, height: 100 });
    }

    #[test]
    fn fill_height_widens_the_viewport_on_a_wide_surface() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 400, height: 100 };
        assert_fill_height_invariants(surface);

        let viewport: Viewport = Viewport::fill_height(CENTER, HALF_HEIGHT, surface);
        assert!((viewport.max.x - viewport.min.x) / 2.0 > HALF_HEIGHT, "wider than tall");
    }

    #[test]
    fn fill_height_narrows_the_viewport_on_a_tall_surface() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 400 };
        assert_fill_height_invariants(surface);

        let viewport: Viewport = Viewport::fill_height(CENTER, HALF_HEIGHT, surface);
        assert!((viewport.max.x - viewport.min.x) / 2.0 < HALF_HEIGHT, "narrower than tall");
    }
}
