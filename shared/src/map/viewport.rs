use std::f64::consts::{PI, TAU};

use crate::map::projection::ProjectedPoint;

/// The smallest half-height, in projected radians, the camera may zoom in to: roughly one degree of
/// latitude filling the surface height. Past this the `f64`->`f32` cast into the viewport uniform loses
/// precision and no country needs more room, and it stops a fast wheel or pinch from driving the view to
/// a degenerate sub-microradian box. Equals `projection::project(0.5, 0.0).y` (half of a one-degree band
/// about the equator), pinned by a test.
const MIN_ZOOM_IN_HALF_HEIGHT: f64 = 0.00872671714852773;

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
    /// extent, horizontally centered on `center_x`, widened by the surface aspect (isotropic, never
    /// stretched). When filling the height would make the width exceed one world turn (`2π`) — a surface
    /// wider than approximately 2:1 for the home framing — the width is capped at `2π` and the view then
    /// shows a vertical slice of the extent instead of the whole of it. Delegates to
    /// `zoom_to_half_height` so the home view and manual zoom share one construction-and-clamp path.
    /// Assumes `surface` dimensions are nonzero and `max_y > min_y`.
    pub fn fill_height(center_x: f64, min_y: f64, max_y: f64, surface: SurfaceDimensions) -> Viewport {
        let requested_half_height: f64 = (max_y - min_y) / 2.0;
        let center_y: f64 = (min_y + max_y) / 2.0;
        let width_cap_half_height: f64 = PI * (surface.height as f64 / surface.width as f64);
        let max_half_height: f64 = requested_half_height.min(width_cap_half_height);

        let seed: Viewport = Viewport {
            min: ProjectedPoint { x: center_x, y: center_y },
            max: ProjectedPoint { x: center_x, y: center_y },
        };

        seed.zoom_to_half_height(requested_half_height, max_half_height, surface)
    }

    /// The construction-and-clamp core: a viewport of the given vertical half-extent (clamped into
    /// `[MIN_ZOOM_IN_HALF_HEIGHT, max_half_height]`), keeping this viewport's center, with the width
    /// re-derived from `surface`'s aspect so the map is never stretched. The width is derived from the
    /// clamped half-height alone, never read from `self`, so a zero-width seed is valid.
    pub fn zoom_to_half_height(&self, half_height: f64, max_half_height: f64, surface: SurfaceDimensions) -> Viewport {
        let clamped_half_height: f64 = clamp_half_height(half_height, max_half_height);
        let center: ProjectedPoint = self.center();
        let half_width: f64 = clamped_half_height * (surface.width as f64 / surface.height as f64);

        Viewport {
            min: ProjectedPoint { x: center.x - half_width, y: center.y - clamped_half_height },
            max: ProjectedPoint { x: center.x + half_width, y: center.y + clamped_half_height },
        }
    }

    /// A viewport zoomed by `factor` (greater than 1 zooms in) about the projected `anchor`, holding that
    /// anchor fixed on screen. The vertical position clamp against the home latitude range is folded into
    /// the recenter, so there is no post-step that could move the anchor: horizontal anchoring is exact
    /// for every zoom, and vertical anchoring is exact except within one range-height of the top/bottom
    /// edge, where the range bound wins and the anchor's screen-y yields by the clamp amount. `factor` is
    /// assumed positive.
    pub fn zoom_about(
        &self,
        factor: f64,
        anchor: ProjectedPoint,
        max_half_height: f64,
        home_min_y: f64,
        home_max_y: f64,
        surface: SurfaceDimensions,
    ) -> Viewport {
        let target_half_height: f64 = self.half_height() / factor;
        let clamped_half_height: f64 = clamp_half_height(target_half_height, max_half_height);

        // The ratio actually achieved after clamping, not the requested `1/factor`, is what keeps the
        // anchor fixed when the zoom is clamped at the ceiling.
        let achieved_ratio: f64 = clamped_half_height / self.half_height();

        let center: ProjectedPoint = self.center();
        let new_center_x: f64 = anchor.x + (center.x - anchor.x) * achieved_ratio;
        let new_center_y: f64 = anchor.y + (center.y - anchor.y) * achieved_ratio;
        let clamped_center_y: f64 = clamp_center_y(new_center_y, clamped_half_height, home_min_y, home_max_y);

        let half_width: f64 = clamped_half_height * (surface.width as f64 / surface.height as f64);

        Viewport {
            min: ProjectedPoint { x: new_center_x - half_width, y: clamped_center_y - clamped_half_height },
            max: ProjectedPoint { x: new_center_x + half_width, y: clamped_center_y + clamped_half_height },
        }
    }

    /// A viewport translated by the projected-space delta `(dx, dy)`. Pure translation: width, height,
    /// and aspect are unchanged, and `x` is not clamped (horizontal wraparound is handled by
    /// `normalize_longitude_turns` and the renderer, not by bounds).
    pub fn pan_by(&self, dx: f64, dy: f64) -> Viewport {
        Viewport {
            min: ProjectedPoint { x: self.min.x + dx, y: self.min.y + dy },
            max: ProjectedPoint { x: self.max.x + dx, y: self.max.y + dy },
        }
    }

    /// This viewport slid vertically so it stays inside the home latitude range `[home_min_y, home_max_y]`,
    /// with its extent unchanged. When the view is at least as tall as the range (full zoom-out) it
    /// centers on the range. Horizontal is untouched.
    pub fn clamp_vertical(&self, home_min_y: f64, home_max_y: f64) -> Viewport {
        let center: ProjectedPoint = self.center();
        let clamped_center_y: f64 = clamp_center_y(center.y, self.half_height(), home_min_y, home_max_y);

        self.pan_by(0.0, clamped_center_y - center.y)
    }

    /// This viewport shifted by whole turns of `2π` along `x` so its center lands in `[-π, π]`. This keeps
    /// the renderer's two-instance antimeridian model valid for arbitrarily long horizontal pans: the
    /// renderer draws only the natural copy plus one copy shifted by one turn, covering `[-π, 3π]`, so a
    /// pan that slid the center far past the seam would otherwise leave the view outside every drawn copy
    /// and blank the map. A whole-turn shift changes the resolved longitude only by a multiple of 360°,
    /// which the hit-test's `wrap_longitude` folds away, so hover and selection are unaffected.
    pub fn normalize_longitude_turns(&self) -> Viewport {
        let center_x: f64 = (self.min.x + self.max.x) / 2.0;
        let turns: f64 = ((center_x + PI) / TAU).floor();

        self.pan_by(-turns * TAU, 0.0)
    }

    fn center(&self) -> ProjectedPoint {
        ProjectedPoint {
            x: (self.min.x + self.max.x) / 2.0,
            y: (self.min.y + self.max.y) / 2.0,
        }
    }

    fn half_height(&self) -> f64 {
        (self.max.y - self.min.y) / 2.0
    }
}

/// Clamps a requested half-height into `[MIN_ZOOM_IN_HALF_HEIGHT, max_half_height]`: the zoom-in floor
/// and the caller-supplied zoom-out ceiling. The `debug_assert` front-runs the `clamp` panic with a
/// legible message if a caller passes a ceiling below the floor.
fn clamp_half_height(requested: f64, max_half_height: f64) -> f64 {
    debug_assert!(
        max_half_height >= MIN_ZOOM_IN_HALF_HEIGHT,
        "the zoom-out ceiling must not fall below the zoom-in floor",
    );

    requested.clamp(MIN_ZOOM_IN_HALF_HEIGHT, max_half_height)
}

/// Clamps a center-y so a view of half-height `half_height` stays inside `[home_min_y, home_max_y]`. When
/// the view is at least as tall as the range the valid interval collapses, so the view centers on the
/// range midpoint rather than letting a floating-point `lo > hi` panic `clamp`.
fn clamp_center_y(center_y: f64, half_height: f64, home_min_y: f64, home_max_y: f64) -> f64 {
    let lo: f64 = home_min_y + half_height;
    let hi: f64 = home_max_y - half_height;

    if lo >= hi {
        (home_min_y + home_max_y) / 2.0
    } else {
        center_y.clamp(lo, hi)
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
    use crate::map::projection;

    const CENTER_X: f64 = 0.5;
    const MIN_Y: f64 = -1.25;
    const MAX_Y: f64 = 0.75; // extent midpoint -0.25, half-height 1.0
    const TOLERANCE: f64 = 1e-12;

    // A home latitude range wide enough that a zoomed-in view slides freely inside it, for the pan and
    // zoom clamp tests.
    const HOME_MIN_Y: f64 = -1.1;
    const HOME_MAX_Y: f64 = 2.0;
    const HOME_HALF_HEIGHT: f64 = (HOME_MAX_Y - HOME_MIN_Y) / 2.0;

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

    fn normalized_screen_position(viewport: Viewport, point: ProjectedPoint) -> (f64, f64) {
        let normalized_x: f64 = (point.x - viewport.min.x) / (viewport.max.x - viewport.min.x);
        // Surface y grows downward, so the top of the view (max projected y) is normalized-y 0.
        let normalized_y: f64 = (viewport.max.y - point.y) / (viewport.max.y - viewport.min.y);

        (normalized_x, normalized_y)
    }

    #[test]
    fn fill_height_fills_vertically_and_matches_aspect_on_a_square_surface() {
        assert_fill_height_invariants(SurfaceDimensions { width: 100, height: 100 });
    }

    #[test]
    fn fill_height_widens_the_viewport_on_a_wide_surface() {
        // 2:1 keeps the test band's width under one world turn, so fill_height still fills vertically; a
        // 4:1 surface would trip the 2π width cap (exercised by the ultrawide test below).
        let surface: SurfaceDimensions = SurfaceDimensions { width: 200, height: 100 };
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

    #[test]
    fn fill_height_caps_the_width_at_one_world_turn_on_an_ultrawide_surface() {
        // 32:9 forces the test band's fill-height width past 2π, so fill_height caps the width and no
        // longer fills the height. Aspect stays locked (no stretch) and the width is exactly one turn.
        let surface: SurfaceDimensions = SurfaceDimensions { width: 320, height: 90 };
        let viewport: Viewport = Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface);
        let width: f64 = viewport.max.x - viewport.min.x;
        let half_width: f64 = width / 2.0;
        let half_height: f64 = (viewport.max.y - viewport.min.y) / 2.0;

        assert!((width - TAU).abs() < 1e-9, "width capped at one world turn");
        assert!(half_height < (MAX_Y - MIN_Y) / 2.0, "no longer fills the full height");
        assert!((half_width / half_height - surface.width as f64 / surface.height as f64).abs() < 1e-9, "aspect still locked");
    }

    #[test]
    fn fill_height_delegates_to_zoom_to_half_height() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 300, height: 100 };
        let requested_half_height: f64 = (MAX_Y - MIN_Y) / 2.0;
        let ceiling: f64 = requested_half_height.min(PI * (surface.height as f64 / surface.width as f64));
        let seed: Viewport = Viewport {
            min: ProjectedPoint { x: CENTER_X, y: (MIN_Y + MAX_Y) / 2.0 },
            max: ProjectedPoint { x: CENTER_X, y: (MIN_Y + MAX_Y) / 2.0 },
        };

        assert_eq!(
            Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface),
            seed.zoom_to_half_height(requested_half_height, ceiling, surface),
        );
    }

    #[test]
    fn min_zoom_in_half_height_is_one_degree_of_latitude() {
        assert!((MIN_ZOOM_IN_HALF_HEIGHT - projection::project(0.5, 0.0).y).abs() < 1e-15);
    }

    #[test]
    fn clamp_half_height_bounds_both_directions() {
        assert_eq!(clamp_half_height(5.0, 2.0), 2.0);
        assert_eq!(clamp_half_height(1e-9, 2.0), MIN_ZOOM_IN_HALF_HEIGHT);
        assert_eq!(clamp_half_height(1.0, 2.0), 1.0);
    }

    #[test]
    #[should_panic(expected = "zoom-out ceiling")]
    fn clamp_half_height_asserts_ceiling_above_floor() {
        let _clamped: f64 = clamp_half_height(1.0, MIN_ZOOM_IN_HALF_HEIGHT / 2.0);
    }

    #[test]
    fn zoom_to_half_height_matches_surface_aspect_from_a_zero_width_seed() {
        for surface in [
            SurfaceDimensions { width: 100, height: 100 },
            SurfaceDimensions { width: 300, height: 100 },
            SurfaceDimensions { width: 100, height: 300 },
        ] {
            let seed: Viewport = Viewport { min: ProjectedPoint { x: 0.3, y: 0.2 }, max: ProjectedPoint { x: 0.3, y: 0.2 } };
            let zoomed: Viewport = seed.zoom_to_half_height(0.5, 2.0, surface);
            let half_width: f64 = (zoomed.max.x - zoomed.min.x) / 2.0;
            let half_height: f64 = (zoomed.max.y - zoomed.min.y) / 2.0;

            assert!((half_height - 0.5).abs() < TOLERANCE, "half-height honored");
            assert!((half_width / half_height - surface.width as f64 / surface.height as f64).abs() < TOLERANCE, "aspect matches");
            assert!(((zoomed.min.x + zoomed.max.x) / 2.0 - 0.3).abs() < TOLERANCE, "center x preserved");
            assert!(((zoomed.min.y + zoomed.max.y) / 2.0 - 0.2).abs() < TOLERANCE, "center y preserved");
        }
    }

    #[test]
    fn zoom_about_holds_the_anchor_fixed_when_unclamped() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 200, height: 100 };
        // Aspect 2 matches the surface (invariant zoom_about assumes).
        let viewport: Viewport = Viewport { min: ProjectedPoint { x: -2.0, y: -1.0 }, max: ProjectedPoint { x: 2.0, y: 1.0 } };
        let anchor: ProjectedPoint = ProjectedPoint { x: 1.0, y: 0.3 };

        let before: (f64, f64) = normalized_screen_position(viewport, anchor);
        let zoomed: Viewport = viewport.zoom_about(2.0, anchor, HOME_HALF_HEIGHT, HOME_MIN_Y, HOME_MAX_Y, surface);
        let after: (f64, f64) = normalized_screen_position(zoomed, anchor);

        assert!((before.0 - after.0).abs() < 1e-9, "anchor screen-x fixed");
        assert!((before.1 - after.1).abs() < 1e-9, "anchor screen-y fixed");
    }

    #[test]
    fn zoom_about_keeps_the_horizontal_anchor_fixed_when_clamped_at_the_ceiling() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 200, height: 100 };
        // Aspect 2 matches the surface.
        let viewport: Viewport = Viewport { min: ProjectedPoint { x: -0.2, y: 0.2 }, max: ProjectedPoint { x: 0.2, y: 0.4 } };
        let anchor: ProjectedPoint = ProjectedPoint { x: 0.15, y: 0.35 };

        // Zoom out hard; the half-height clamps to the ceiling but the horizontal anchor must not drift.
        let zoomed: Viewport = viewport.zoom_about(0.01, anchor, HOME_HALF_HEIGHT, HOME_MIN_Y, HOME_MAX_Y, surface);

        assert!(((zoomed.max.y - zoomed.min.y) / 2.0 - HOME_HALF_HEIGHT).abs() < 1e-9, "half-height at the ceiling");
        let before_x: f64 = normalized_screen_position(viewport, anchor).0;
        let after_x: f64 = normalized_screen_position(zoomed, anchor).0;
        assert!((before_x - after_x).abs() < 1e-9, "horizontal anchor fixed at the ceiling");
    }

    #[test]
    fn zoom_about_pins_the_view_to_the_range_edge_and_yields_vertical_anchor() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 100 };
        // A small view near the top of the home range; zooming out about a near-top anchor grows the box
        // past home_max_y, so the range bound wins and the box top sits exactly at home_max_y.
        let viewport: Viewport = Viewport { min: ProjectedPoint { x: -0.1, y: 1.7 }, max: ProjectedPoint { x: 0.1, y: 1.9 } };
        let anchor: ProjectedPoint = ProjectedPoint { x: 0.0, y: 1.88 };

        let zoomed: Viewport = viewport.zoom_about(0.1, anchor, HOME_HALF_HEIGHT, HOME_MIN_Y, HOME_MAX_Y, surface);

        assert!(zoomed.max.y <= HOME_MAX_Y + 1e-12, "box top stays within the range");
        assert!((zoomed.max.y - HOME_MAX_Y).abs() < 1e-9, "box top pinned to the range edge");
    }

    #[test]
    fn pan_by_is_a_pure_translation() {
        let viewport: Viewport = Viewport { min: ProjectedPoint { x: 3.0, y: -0.5 }, max: ProjectedPoint { x: 3.4, y: 0.5 } };
        let panned: Viewport = viewport.pan_by(0.2, -0.1);

        assert_eq!(panned.min, ProjectedPoint { x: 3.2, y: -0.6 });
        assert_eq!(panned.max, ProjectedPoint { x: 3.6, y: 0.4 });
    }

    #[test]
    fn clamp_vertical_pulls_the_view_inside_the_range() {
        let above: Viewport = Viewport { min: ProjectedPoint { x: 0.0, y: 1.8 }, max: ProjectedPoint { x: 1.0, y: 2.3 } };
        let clamped_above: Viewport = above.clamp_vertical(HOME_MIN_Y, HOME_MAX_Y);
        assert!((clamped_above.max.y - HOME_MAX_Y).abs() < TOLERANCE, "top pulled to the range top");
        assert!((clamped_above.max.y - clamped_above.min.y - 0.5).abs() < TOLERANCE, "extent unchanged");

        let below: Viewport = Viewport { min: ProjectedPoint { x: 0.0, y: -1.6 }, max: ProjectedPoint { x: 1.0, y: -1.1 } };
        let clamped_below: Viewport = below.clamp_vertical(HOME_MIN_Y, HOME_MAX_Y);
        assert!((clamped_below.min.y - HOME_MIN_Y).abs() < TOLERANCE, "bottom pulled to the range bottom");

        let inside: Viewport = Viewport { min: ProjectedPoint { x: 0.0, y: 0.0 }, max: ProjectedPoint { x: 1.0, y: 0.5 } };
        assert_eq!(inside.clamp_vertical(HOME_MIN_Y, HOME_MAX_Y), inside, "already inside is untouched");
    }

    #[test]
    fn clamp_vertical_centers_a_full_range_height_view() {
        let full: Viewport = Viewport { min: ProjectedPoint { x: 0.0, y: -3.0 }, max: ProjectedPoint { x: 1.0, y: -3.0 + 2.0 * HOME_HALF_HEIGHT } };
        let clamped: Viewport = full.clamp_vertical(HOME_MIN_Y, HOME_MAX_Y);

        assert!((clamped.min.y - HOME_MIN_Y).abs() < TOLERANCE, "centered on the range bottom");
        assert!((clamped.max.y - HOME_MAX_Y).abs() < TOLERANCE, "centered on the range top");
    }

    #[test]
    fn normalize_longitude_turns_recenters_within_one_turn() {
        let far_east: Viewport = Viewport { min: ProjectedPoint { x: 19.0, y: -0.5 }, max: ProjectedPoint { x: 21.0, y: 0.5 } };
        let normalized: Viewport = far_east.normalize_longitude_turns();
        let center_x: f64 = (normalized.min.x + normalized.max.x) / 2.0;

        assert!(center_x >= -PI && center_x < PI, "center within one turn of the seam");
        assert!((normalized.max.x - normalized.min.x - 2.0).abs() < TOLERANCE, "width preserved");
        let shift: f64 = far_east.min.x - normalized.min.x;
        assert!((shift / TAU - (shift / TAU).round()).abs() < 1e-12, "shift is a whole number of turns");

        let inside: Viewport = Viewport { min: ProjectedPoint { x: -0.5, y: -0.5 }, max: ProjectedPoint { x: 0.5, y: 0.5 } };
        assert_eq!(inside.normalize_longitude_turns(), inside, "already within one turn is untouched");
    }
}
