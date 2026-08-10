use std::f64::consts::{PI, TAU};

use crate::map::projection::ProjectedPoint;

/// The smallest height, in projected radians, the camera may zoom in to: roughly an eight-degree band of
/// latitude (a country and its neighbors) filling the surface. This caps zoom-in so the atlas stays at a
/// regional scale, and past a much tighter bound the `f64`->`f32` cast into the viewport uniform loses
/// precision. Equals the projected height of an eight-degree latitude band about the equator
/// (`project(4.0, 0.0).y - project(-4.0, 0.0).y`), pinned by a test.
const MIN_ZOOM_IN_HEIGHT: f64 = 0.13969898581435658;

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
    /// extent, horizontally centered on `center_x` (isotropic, never stretched). Exception: the width
    /// never exceeds one world turn (`2π`), so on a surface too wide to fit the extent within one turn the
    /// width is capped and the view then covers less than the full extent.
    pub fn fill_height(center_x: f64, min_y: f64, max_y: f64, surface: SurfaceDimensions) -> Viewport {
        let requested_height: f64 = max_y - min_y;
        let center_y: f64 = (min_y + max_y) / 2.0;
        let width_cap_height: f64 = TAU * (surface.height as f64 / surface.width as f64);
        let max_height: f64 = requested_height.min(width_cap_height);

        let seed: Viewport = Viewport {
            min: ProjectedPoint { x: center_x, y: center_y },
            max: ProjectedPoint { x: center_x, y: center_y },
        };

        seed.zoom_to_height(requested_height, max_height, surface)
    }

    /// The construction-and-clamp core: a viewport of the given visible height (clamped into
    /// `[MIN_ZOOM_IN_HEIGHT, max_height]`), keeping this viewport's center, with the width re-derived from
    /// `surface`'s aspect so the map is never stretched. The width is derived from the clamped height
    /// alone, never read from `self`, so a zero-width seed is valid.
    pub fn zoom_to_height(&self, target_height: f64, max_height: f64, surface: SurfaceDimensions) -> Viewport {
        let clamped_height: f64 = clamp_height(target_height, max_height);
        let center: ProjectedPoint = self.center();
        let width: f64 = clamped_height * (surface.width as f64 / surface.height as f64);

        Viewport {
            min: ProjectedPoint { x: center.x - width / 2.0, y: center.y - clamped_height / 2.0 },
            max: ProjectedPoint { x: center.x + width / 2.0, y: center.y + clamped_height / 2.0 },
        }
    }

    /// A viewport zoomed by `factor` (greater than 1 zooms in) about the projected `anchor`, holding that
    /// anchor fixed on screen. The vertical position clamp against the home latitude range is folded into
    /// the recenter, so there is no post-step that could move the anchor: horizontal anchoring is exact
    /// for every zoom, and vertical anchoring is exact except within half a range-height of the top/bottom
    /// edge, where the range bound wins and the anchor's screen-y yields by the clamp amount. `factor` is
    /// assumed positive.
    pub fn zoom_about(
        &self,
        factor: f64,
        anchor: ProjectedPoint,
        max_height: f64,
        min_y: f64,
        max_y: f64,
        surface: SurfaceDimensions,
    ) -> Viewport {
        let target_height: f64 = self.height() / factor;
        let clamped_height: f64 = clamp_height(target_height, max_height);

        // The ratio actually achieved after clamping, not the requested `1/factor`, is what keeps the
        // anchor fixed when the zoom is clamped at the ceiling.
        let achieved_ratio: f64 = clamped_height / self.height();

        let center: ProjectedPoint = self.center();
        let new_center_x: f64 = anchor.x + (center.x - anchor.x) * achieved_ratio;
        let new_center_y: f64 = anchor.y + (center.y - anchor.y) * achieved_ratio;
        let clamped_center_y: f64 = clamp_center_y(new_center_y, clamped_height, min_y, max_y);

        let width: f64 = clamped_height * (surface.width as f64 / surface.height as f64);

        Viewport {
            min: ProjectedPoint { x: new_center_x - width / 2.0, y: clamped_center_y - clamped_height / 2.0 },
            max: ProjectedPoint { x: new_center_x + width / 2.0, y: clamped_center_y + clamped_height / 2.0 },
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

    /// This viewport slid vertically so it stays inside the home latitude range `[min_y, max_y]`,
    /// with its height unchanged. When the view is at least as tall as the range (full zoom-out) it
    /// centers on the range. Horizontal is untouched.
    pub fn clamp_vertical(&self, min_y: f64, max_y: f64) -> Viewport {
        let center: ProjectedPoint = self.center();
        let clamped_center_y: f64 = clamp_center_y(center.y, self.height(), min_y, max_y);

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

    /// This view re-fitted to a new surface: the center and zoom level (the visible height) are kept while
    /// the width is re-derived from the new aspect, then the zoom-out ceiling, the home latitude range,
    /// and the antimeridian normalization are re-applied. Called on a surface resize or a
    /// device-pixel-ratio change so the user's pan/zoom is preserved rather than reset to the home view.
    pub fn refit_to_surface(&self, surface: SurfaceDimensions, max_height: f64, min_y: f64, max_y: f64) -> Viewport {
        self.zoom_to_height(self.height(), max_height, surface)
            .clamp_vertical(min_y, max_y)
            .normalize_longitude_turns()
    }

    /// A viewport containing the projected rectangle `[min_x, max_x] x [min_y, max_y]` centered on
    /// `center`, with `margin_fraction` padding, at the surface aspect and never stretched. The
    /// counterpart to `fill_height`: `fill_height` frames only the vertical extent (letting the
    /// horizontal overflow); this frames the whole rectangle by taking whichever of its height or its
    /// width-over-aspect is larger, so both axes fit. The framed height is floored at `min_height` (so a
    /// small rectangle still lands at a legible zoom-out rather than filling the view) and then clamped
    /// into `[MIN_ZOOM_IN_HEIGHT, max_height]`.
    pub fn fit_bounds(
        min_x: f64,
        max_x: f64,
        min_y: f64,
        max_y: f64,
        center: ProjectedPoint,
        margin_fraction: f64,
        min_height: f64,
        max_height: f64,
        surface: SurfaceDimensions,
    ) -> Viewport {
        let aspect: f64 = surface.width as f64 / surface.height as f64;
        let contain_height: f64 = (max_y - min_y).max((max_x - min_x) / aspect);
        let padded_height: f64 = (contain_height * (1.0 + margin_fraction)).max(min_height);

        let seed: Viewport = Viewport { min: center, max: center };

        seed.zoom_to_height(padded_height, max_height, surface)
    }

    /// This viewport blended toward `target` by eased `t` in `[0, 1]`, interpolating the center and the
    /// visible height and re-deriving the width from the surface aspect so no intermediate frame is
    /// stretched. Height blends geometrically (constant perceived zoom rate); center linearly. The
    /// horizontal blend takes the short way across the antimeridian: the target center-x is unwrapped to
    /// within half a world turn of this center-x first.
    pub fn interpolate_to(&self, target: Viewport, t: f64, surface: SurfaceDimensions) -> Viewport {
        let from_center: ProjectedPoint = self.center();
        let target_center: ProjectedPoint = target.center();

        let center_x: f64 = lerp(from_center.x, unwrap_nearest(from_center.x, target_center.x), t);
        let center_y: f64 = lerp(from_center.y, target_center.y, t);
        let height: f64 = geometric_lerp(self.height(), target.height(), t);

        let seed: Viewport = Viewport {
            min: ProjectedPoint { x: center_x, y: center_y },
            max: ProjectedPoint { x: center_x, y: center_y },
        };

        seed.zoom_to_height(height, height, surface)
    }

    fn center(&self) -> ProjectedPoint {
        ProjectedPoint {
            x: (self.min.x + self.max.x) / 2.0,
            y: (self.min.y + self.max.y) / 2.0,
        }
    }

    fn height(&self) -> f64 {
        self.max.y - self.min.y
    }
}

/// Clamps a requested height into `[MIN_ZOOM_IN_HEIGHT, max_height]`.
fn clamp_height(requested: f64, max_height: f64) -> f64 {
    debug_assert!(
        max_height >= MIN_ZOOM_IN_HEIGHT,
        "the zoom-out ceiling must not fall below the zoom-in floor",
    );

    requested.clamp(MIN_ZOOM_IN_HEIGHT, max_height)
}

/// Clamps a center-y so a view of the given height stays inside `[min_y, max_y]`. When the view
/// is at least as tall as the range the valid interval collapses, so the view centers on the range
/// midpoint rather than letting a floating-point `lo > hi` panic `clamp`.
fn clamp_center_y(center_y: f64, height: f64, min_y: f64, max_y: f64) -> f64 {
    let lo: f64 = min_y + height / 2.0;
    let hi: f64 = max_y - height / 2.0;

    if lo >= hi {
        (min_y + max_y) / 2.0
    } else {
        center_y.clamp(lo, hi)
    }
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

/// A blend linear in the logarithm, so the ratio between successive `t` steps is constant: a uniform
/// perceived zoom rate across a large scale change. Both `from` and `to` are strictly positive here (a
/// viewport height is at least `MIN_ZOOM_IN_HEIGHT`).
fn geometric_lerp(from: f64, to: f64, t: f64) -> f64 {
    from * (to / from).powf(t)
}

/// `target` shifted by whole turns of `2π` to land within ±π of `reference`: the representation of the
/// same longitude reachable by the shortest horizontal move.
fn unwrap_nearest(reference: f64, target: f64) -> f64 {
    target - ((target - reference) / TAU).round() * TAU
}

/// Physical device pixels: the platform shell multiplies the CSS-pixel cursor position by
/// `devicePixelRatio` so the point shares the device-pixel space of the render surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePoint {
    pub x: f64,
    pub y: f64,
}

impl SurfacePoint {
    pub fn midpoint(self, other: SurfacePoint) -> SurfacePoint {
        SurfacePoint { x: (self.x + other.x) / 2.0, y: (self.y + other.y) / 2.0 }
    }

    pub fn euclidean_distance(self, other: SurfacePoint) -> f64 {
        let dx: f64 = self.x - other.x;
        let dy: f64 = self.y - other.y;

        (dx * dx + dy * dy).sqrt()
    }
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
    const MAX_Y: f64 = 0.75; // extent midpoint -0.25, height 2.0
    const TOLERANCE: f64 = 1e-12;

    // A home latitude range wide enough that a zoomed-in view slides freely inside it, for the pan and
    // zoom clamp tests.
    const HOME_MIN_Y: f64 = -1.1;
    const HOME_MAX_Y: f64 = 2.0;
    const HOME_HEIGHT: f64 = HOME_MAX_Y - HOME_MIN_Y;

    fn assert_fill_height_invariants(surface: SurfaceDimensions) {
        let viewport: Viewport = Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface);
        let width: f64 = viewport.max.x - viewport.min.x;
        let height: f64 = viewport.max.y - viewport.min.y;
        let surface_aspect: f64 = surface.width as f64 / surface.height as f64;

        assert!((viewport.min.y - MIN_Y).abs() < TOLERANCE, "min_y fills the surface bottom");
        assert!((viewport.max.y - MAX_Y).abs() < TOLERANCE, "max_y fills the surface top");
        assert!((width / height - surface_aspect).abs() < TOLERANCE, "aspect matches surface");
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
        assert!(viewport.max.x - viewport.min.x > MAX_Y - MIN_Y, "wider than tall");
    }

    #[test]
    fn fill_height_narrows_the_viewport_on_a_tall_surface() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 400 };
        assert_fill_height_invariants(surface);

        let viewport: Viewport = Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface);
        assert!(viewport.max.x - viewport.min.x < MAX_Y - MIN_Y, "narrower than tall");
    }

    #[test]
    fn fill_height_caps_the_width_at_one_world_turn_on_an_ultrawide_surface() {
        // 32:9 forces the test band's fill-height width past 2π, so fill_height caps the width and no
        // longer fills the height. Aspect stays locked (no stretch) and the width is exactly one turn.
        let surface: SurfaceDimensions = SurfaceDimensions { width: 320, height: 90 };
        let viewport: Viewport = Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface);
        let width: f64 = viewport.max.x - viewport.min.x;
        let height: f64 = viewport.max.y - viewport.min.y;

        assert!((width - TAU).abs() < 1e-9, "width capped at one world turn");
        assert!(height < MAX_Y - MIN_Y, "no longer fills the full height");
        assert!((width / height - surface.width as f64 / surface.height as f64).abs() < 1e-9, "aspect still locked");
    }

    #[test]
    fn fill_height_delegates_to_zoom_to_height() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 300, height: 100 };
        let requested_height: f64 = MAX_Y - MIN_Y;
        let ceiling: f64 = requested_height.min(TAU * (surface.height as f64 / surface.width as f64));
        let seed: Viewport = Viewport {
            min: ProjectedPoint { x: CENTER_X, y: (MIN_Y + MAX_Y) / 2.0 },
            max: ProjectedPoint { x: CENTER_X, y: (MIN_Y + MAX_Y) / 2.0 },
        };

        assert_eq!(
            Viewport::fill_height(CENTER_X, MIN_Y, MAX_Y, surface),
            seed.zoom_to_height(requested_height, ceiling, surface),
        );
    }

    #[test]
    fn min_zoom_in_height_is_an_eight_degree_band_of_latitude() {
        let eight_degree_height: f64 = projection::project(4.0, 0.0).y - projection::project(-4.0, 0.0).y;
        assert!((MIN_ZOOM_IN_HEIGHT - eight_degree_height).abs() < 1e-15);
    }

    #[test]
    fn clamp_height_bounds_both_directions() {
        assert_eq!(clamp_height(5.0, 2.0), 2.0);
        assert_eq!(clamp_height(1e-9, 2.0), MIN_ZOOM_IN_HEIGHT);
        assert_eq!(clamp_height(1.0, 2.0), 1.0);
    }

    #[test]
    #[should_panic(expected = "zoom-out ceiling")]
    fn clamp_height_asserts_ceiling_above_floor() {
        let _clamped: f64 = clamp_height(1.0, MIN_ZOOM_IN_HEIGHT / 2.0);
    }

    #[test]
    fn zoom_to_height_matches_surface_aspect_from_a_zero_width_seed() {
        for surface in [
            SurfaceDimensions { width: 100, height: 100 },
            SurfaceDimensions { width: 300, height: 100 },
            SurfaceDimensions { width: 100, height: 300 },
        ] {
            let seed: Viewport = Viewport { min: ProjectedPoint { x: 0.3, y: 0.2 }, max: ProjectedPoint { x: 0.3, y: 0.2 } };
            let zoomed: Viewport = seed.zoom_to_height(1.0, 4.0, surface);
            let width: f64 = zoomed.max.x - zoomed.min.x;
            let height: f64 = zoomed.max.y - zoomed.min.y;

            assert!((height - 1.0).abs() < TOLERANCE, "height honored");
            assert!((width / height - surface.width as f64 / surface.height as f64).abs() < TOLERANCE, "aspect matches");
            assert!(((zoomed.min.x + zoomed.max.x) / 2.0 - 0.3).abs() < TOLERANCE, "center x preserved");
            assert!(((zoomed.min.y + zoomed.max.y) / 2.0 - 0.2).abs() < TOLERANCE, "center y preserved");
        }
    }

    #[test]
    fn zoom_about_holds_the_anchor_fixed_when_unclamped() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 200, height: 100 };
        // Aspect 2 matches the surface (the invariant zoom_about assumes).
        let viewport: Viewport = Viewport { min: ProjectedPoint { x: -2.0, y: -1.0 }, max: ProjectedPoint { x: 2.0, y: 1.0 } };
        let anchor: ProjectedPoint = ProjectedPoint { x: 1.0, y: 0.3 };

        let before: (f64, f64) = normalized_screen_position(viewport, anchor);
        let zoomed: Viewport = viewport.zoom_about(2.0, anchor, HOME_HEIGHT, HOME_MIN_Y, HOME_MAX_Y, surface);
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

        // Zoom out hard; the height clamps to the ceiling but the horizontal anchor must not drift.
        let zoomed: Viewport = viewport.zoom_about(0.01, anchor, HOME_HEIGHT, HOME_MIN_Y, HOME_MAX_Y, surface);

        assert!((zoomed.max.y - zoomed.min.y - HOME_HEIGHT).abs() < 1e-9, "height at the ceiling");
        let before_x: f64 = normalized_screen_position(viewport, anchor).0;
        let after_x: f64 = normalized_screen_position(zoomed, anchor).0;
        assert!((before_x - after_x).abs() < 1e-9, "horizontal anchor fixed at the ceiling");
    }

    #[test]
    fn zoom_about_pins_the_view_to_the_range_edge_and_yields_vertical_anchor() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 100 };
        // A small view near the top of the home range; zooming out about a near-top anchor grows the box
        // past max_y, so the range bound wins and the box top sits exactly at max_y.
        let viewport: Viewport = Viewport { min: ProjectedPoint { x: -0.1, y: 1.7 }, max: ProjectedPoint { x: 0.1, y: 1.9 } };
        let anchor: ProjectedPoint = ProjectedPoint { x: 0.0, y: 1.88 };

        let zoomed: Viewport = viewport.zoom_about(0.1, anchor, HOME_HEIGHT, HOME_MIN_Y, HOME_MAX_Y, surface);

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
        assert!((clamped_above.max.y - clamped_above.min.y - 0.5).abs() < TOLERANCE, "height unchanged");

        let below: Viewport = Viewport { min: ProjectedPoint { x: 0.0, y: -1.6 }, max: ProjectedPoint { x: 1.0, y: -1.1 } };
        let clamped_below: Viewport = below.clamp_vertical(HOME_MIN_Y, HOME_MAX_Y);
        assert!((clamped_below.min.y - HOME_MIN_Y).abs() < TOLERANCE, "bottom pulled to the range bottom");

        let inside: Viewport = Viewport { min: ProjectedPoint { x: 0.0, y: 0.0 }, max: ProjectedPoint { x: 1.0, y: 0.5 } };
        assert_eq!(inside.clamp_vertical(HOME_MIN_Y, HOME_MAX_Y), inside, "already inside is untouched");
    }

    #[test]
    fn clamp_vertical_centers_a_full_range_height_view() {
        let full: Viewport = Viewport { min: ProjectedPoint { x: 0.0, y: -3.0 }, max: ProjectedPoint { x: 1.0, y: -3.0 + HOME_HEIGHT } };
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

    #[test]
    fn refit_to_surface_preserves_center_and_zoom() {
        // A zoomed-in, off-center view at aspect 2 (matching a 200x100 surface): height 0.8, width 1.6.
        let viewport: Viewport = Viewport { min: ProjectedPoint { x: -0.8, y: -0.1 }, max: ProjectedPoint { x: 0.8, y: 0.7 } };
        let center_x: f64 = 0.0;
        let center_y: f64 = 0.3;
        let height: f64 = 0.8;

        // A device-pixel-ratio change scales the surface but keeps the aspect, so the view is unchanged.
        let same_aspect: Viewport = viewport.refit_to_surface(SurfaceDimensions { width: 400, height: 200 }, 4.0, HOME_MIN_Y, HOME_MAX_Y);
        assert!(((same_aspect.min.x + same_aspect.max.x) / 2.0 - center_x).abs() < TOLERANCE, "center x kept");
        assert!(((same_aspect.min.y + same_aspect.max.y) / 2.0 - center_y).abs() < TOLERANCE, "center y kept");
        assert!((same_aspect.max.y - same_aspect.min.y - height).abs() < TOLERANCE, "height kept");
        assert!((same_aspect.max.x - same_aspect.min.x - 1.6).abs() < TOLERANCE, "width kept at same aspect");

        // A window resize to a square surface keeps the center and height but re-derives the width.
        let new_aspect: Viewport = viewport.refit_to_surface(SurfaceDimensions { width: 100, height: 100 }, 4.0, HOME_MIN_Y, HOME_MAX_Y);
        assert!(((new_aspect.min.x + new_aspect.max.x) / 2.0 - center_x).abs() < TOLERANCE, "center x kept across aspect change");
        assert!((new_aspect.max.y - new_aspect.min.y - height).abs() < TOLERANCE, "height kept across aspect change");
        assert!((new_aspect.max.x - new_aspect.min.x - height).abs() < TOLERANCE, "width re-derived from the square aspect");
    }

    fn viewport_aspect(viewport: Viewport) -> f64 {
        (viewport.max.x - viewport.min.x) / (viewport.max.y - viewport.min.y)
    }

    #[test]
    fn geometric_lerp_endpoints_and_constant_ratio() {
        assert!((geometric_lerp(2.0, 8.0, 0.0) - 2.0).abs() < TOLERANCE);
        assert!((geometric_lerp(2.0, 8.0, 1.0) - 8.0).abs() < TOLERANCE);
        // Constant ratio: the midpoint is the geometric mean, so it squares to the product of the ends.
        let mid: f64 = geometric_lerp(2.0, 8.0, 0.5);
        assert!((mid - 4.0).abs() < 1e-9, "geometric midpoint of 2 and 8 is 4");
    }

    #[test]
    fn unwrap_nearest_shifts_to_the_near_representative() {
        // -3 rad is more than half a turn from +3 rad; the near representative is +3.28 (one turn up).
        let unwrapped: f64 = unwrap_nearest(3.0, -3.0);
        assert!((unwrapped - (-3.0 + TAU)).abs() < TOLERANCE);
        assert!((unwrap_nearest(0.5, 0.7) - 0.7).abs() < TOLERANCE, "already within a turn is unchanged");
        let shift: f64 = unwrap_nearest(3.0, -3.0) - (-3.0);
        assert!((shift / TAU - (shift / TAU).round()).abs() < 1e-12, "shift is a whole number of turns");
    }

    #[test]
    fn fit_bounds_contains_the_rectangle_with_margin_and_matches_aspect() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 200, height: 100 };
        let fitted: Viewport = Viewport::fit_bounds(-1.0, 1.0, -0.5, 0.5, ProjectedPoint { x: 0.0, y: 0.0 }, 0.1, MIN_ZOOM_IN_HEIGHT, 4.0, surface);

        assert!(fitted.min.x <= -1.0 && fitted.max.x >= 1.0, "contains the rectangle horizontally");
        assert!(fitted.min.y <= -0.5 && fitted.max.y >= 0.5, "contains the rectangle vertically");
        assert!((viewport_aspect(fitted) - 2.0).abs() < TOLERANCE, "aspect matches the surface");
        assert!(((fitted.min.x + fitted.max.x) / 2.0).abs() < TOLERANCE, "centered x");
        // width 2 governs at aspect 2 (2/2 == height 1); padded height is 1.1.
        assert!((fitted.max.y - fitted.min.y - 1.1).abs() < TOLERANCE, "framed to the padded governing extent");
    }

    #[test]
    fn fit_bounds_frames_by_height_for_a_tall_rectangle() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 200, height: 100 };
        let fitted: Viewport = Viewport::fit_bounds(-0.1, 0.1, -2.0, 2.0, ProjectedPoint { x: 0.0, y: 0.0 }, 0.1, MIN_ZOOM_IN_HEIGHT, 8.0, surface);
        // Height 4 governs (0.2/2 == 0.1 < 4); padded to 4.4.
        assert!((fitted.max.y - fitted.min.y - 4.4).abs() < TOLERANCE);
    }

    #[test]
    fn fit_bounds_centers_on_the_passed_center_not_the_rectangle_center() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 100 };
        let fitted: Viewport = Viewport::fit_bounds(-1.0, 1.0, -1.0, 1.0, ProjectedPoint { x: 5.0, y: 5.0 }, 0.1, MIN_ZOOM_IN_HEIGHT, 8.0, surface);

        assert!(((fitted.min.x + fitted.max.x) / 2.0 - 5.0).abs() < TOLERANCE, "centered on the passed center x");
        assert!(((fitted.min.y + fitted.max.y) / 2.0 - 5.0).abs() < TOLERANCE, "centered on the passed center y");
    }

    #[test]
    fn fit_bounds_floors_a_small_rectangle_at_min_height() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 100 };
        // A tiny rectangle whose padded extent (approx. 0.11) is below the 0.6 floor: the floor governs.
        let fitted: Viewport = Viewport::fit_bounds(-0.05, 0.05, -0.05, 0.05, ProjectedPoint { x: 0.0, y: 0.0 }, 0.1, 0.6, 4.0, surface);

        assert!((fitted.max.y - fitted.min.y - 0.6).abs() < TOLERANCE, "framed to the min-height floor, not the tiny extent");
    }

    #[test]
    fn fit_bounds_clamps_a_degenerate_rectangle_to_the_zoom_in_floor() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 100 };
        let fitted: Viewport = Viewport::fit_bounds(0.3, 0.3, 0.2, 0.2, ProjectedPoint { x: 0.3, y: 0.2 }, 0.1, MIN_ZOOM_IN_HEIGHT, 8.0, surface);

        assert!((fitted.max.y - fitted.min.y - MIN_ZOOM_IN_HEIGHT).abs() < TOLERANCE, "clamped to the zoom-in floor");
    }

    #[test]
    fn interpolate_to_holds_aspect_and_hits_the_endpoints() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 200, height: 100 };
        let from: Viewport = Viewport { min: ProjectedPoint { x: -1.0, y: -0.5 }, max: ProjectedPoint { x: 1.0, y: 0.5 } };
        let target: Viewport = Viewport { min: ProjectedPoint { x: 1.5, y: 0.15 }, max: ProjectedPoint { x: 2.5, y: 0.65 } };

        let at_start: Viewport = from.interpolate_to(target, 0.0, surface);
        assert!(((at_start.min.x + at_start.max.x) / 2.0).abs() < 1e-9 && (at_start.max.y - at_start.min.y - 1.0).abs() < 1e-9, "t=0 is the from view");

        let at_end: Viewport = from.interpolate_to(target, 1.0, surface);
        assert!(((at_end.min.x + at_end.max.x) / 2.0 - 2.0).abs() < 1e-9, "t=1 center x is the target");
        assert!((at_end.max.y - at_end.min.y - 0.5).abs() < 1e-9, "t=1 height is the target");

        let midway: Viewport = from.interpolate_to(target, 0.5, surface);
        assert!((viewport_aspect(midway) - 2.0).abs() < 1e-9, "aspect held at the surface aspect");
        assert!(((midway.min.x + midway.max.x) / 2.0 - 1.0).abs() < 1e-9, "center x is the linear blend");
        assert!((midway.max.y - midway.min.y - (0.5_f64).sqrt()).abs() < 1e-9, "height is the geometric blend");
    }

    #[test]
    fn interpolate_to_takes_the_short_way_across_the_antimeridian() {
        let surface: SurfaceDimensions = SurfaceDimensions { width: 100, height: 100 };
        let from: Viewport = Viewport { min: ProjectedPoint { x: 2.5, y: -0.5 }, max: ProjectedPoint { x: 3.5, y: 0.5 } };
        let target: Viewport = Viewport { min: ProjectedPoint { x: -3.5, y: -0.5 }, max: ProjectedPoint { x: -2.5, y: 0.5 } };

        let midway: Viewport = from.interpolate_to(target, 0.5, surface);
        let center_x: f64 = (midway.min.x + midway.max.x) / 2.0;
        assert!(center_x > 3.0, "swept the short way past the seam, not back through zero");
    }
}
