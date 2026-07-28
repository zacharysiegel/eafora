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
    /// The smallest viewport with the surface's aspect ratio that contains `center ± half_width` in x and
    /// `center ± half_height` in y. Whichever axis has surplus relative to the surface shape is expanded,
    /// never cropping the requested content, so the projected content is never stretched. The home view
    /// and zoom-to-country both frame their target through this. Assumes `surface` dimensions are nonzero.
    pub fn fit(center: ProjectedPoint, half_width: f64, half_height: f64, surface: SurfaceDimensions) -> Viewport {
        let surface_aspect: f64 = surface.width as f64 / surface.height as f64;
        let content_aspect: f64 = half_width / half_height;

        let (fitted_half_width, fitted_half_height): (f64, f64) = if surface_aspect >= content_aspect {
            (half_height * surface_aspect, half_height)
        } else {
            (half_width, half_width / surface_aspect)
        };

        Viewport {
            min: ProjectedPoint { x: center.x - fitted_half_width, y: center.y - fitted_half_height },
            max: ProjectedPoint { x: center.x + fitted_half_width, y: center.y + fitted_half_height },
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
    const CONTENT_HALF_WIDTH: f64 = 2.0;
    const CONTENT_HALF_HEIGHT: f64 = 1.0; // content aspect 2.0
    const TOLERANCE: f64 = 1e-12;

    fn assert_fit_invariants(surface: SurfaceDimensions) {
        let viewport: Viewport = Viewport::fit(CENTER, CONTENT_HALF_WIDTH, CONTENT_HALF_HEIGHT, surface);
        let half_width: f64 = (viewport.max.x - viewport.min.x) / 2.0;
        let half_height: f64 = (viewport.max.y - viewport.min.y) / 2.0;

        let surface_aspect: f64 = surface.width as f64 / surface.height as f64;
        assert!((half_width / half_height - surface_aspect).abs() < TOLERANCE, "aspect {half_width}/{half_height}");
        assert!(half_width >= CONTENT_HALF_WIDTH - TOLERANCE, "contains width");
        assert!(half_height >= CONTENT_HALF_HEIGHT - TOLERANCE, "contains height");
        assert!(((viewport.min.x + viewport.max.x) / 2.0 - CENTER.x).abs() < TOLERANCE, "centered x");
        assert!(((viewport.min.y + viewport.max.y) / 2.0 - CENTER.y).abs() < TOLERANCE, "centered y");
    }

    #[test]
    fn fit_matches_aspect_and_contains_on_a_square_surface() {
        assert_fit_invariants(SurfaceDimensions { width: 100, height: 100 });
    }

    #[test]
    fn fit_expands_horizontally_on_a_wide_surface() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 400, height: 100 }; // aspect 4 > content 2
        assert_fit_invariants(surface);

        let viewport: Viewport = Viewport::fit(CENTER, CONTENT_HALF_WIDTH, CONTENT_HALF_HEIGHT, surface);
        assert!(((viewport.max.y - viewport.min.y) / 2.0 - CONTENT_HALF_HEIGHT).abs() < TOLERANCE, "height pinned");
        assert!((viewport.max.x - viewport.min.x) / 2.0 > CONTENT_HALF_WIDTH, "width expanded");
    }

    #[test]
    fn fit_expands_vertically_on_a_tall_surface() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 400 }; // aspect 0.25 < content 2
        assert_fit_invariants(surface);

        let viewport: Viewport = Viewport::fit(CENTER, CONTENT_HALF_WIDTH, CONTENT_HALF_HEIGHT, surface);
        assert!(((viewport.max.x - viewport.min.x) / 2.0 - CONTENT_HALF_WIDTH).abs() < TOLERANCE, "width pinned");
        assert!((viewport.max.y - viewport.min.y) / 2.0 > CONTENT_HALF_HEIGHT, "height expanded");
    }
}
