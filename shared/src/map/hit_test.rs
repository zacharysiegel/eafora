//! Surface-to-viewport mapping: convert a device-pixel point through the viewport into Miller projected
//! space, resolve the region under it (inverse-project, then point-in-polygon against the geometry
//! layer), and apply pan, wheel-zoom, and two-finger pinch gestures to the viewport. The gesture math is
//! pure and shares one surface normalization with the hit-test.

use crate::artifact::geometry::{BoundingBox, CountryFeature, GeometryLayer};
use crate::map::projection::{self, GeoPoint, ProjectedPoint};
use crate::map::{RegionCode, SurfacePoint, SurfaceDimensions, Viewport};

/// A hit-test result: the region under the cursor plus the fields a caller needs (`iso3`, `name_en`),
/// resolved here so callers do not re-parse the geometry layer to recover them.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionHit {
    pub region_code: RegionCode,
    pub iso3: String,
    pub name_en: String,
}

/// The region whose polygon contains `surface_point`, or `None` when the point is off every country
/// (open ocean) or off the map. `surface_dimensions` is required because `surface_point` is in device
/// pixels: the point is normalized against the surface extent before it can be mapped through the
/// viewport. A longitude past the ±180 seam (from a pan across the antimeridian) is wrapped back
/// into range so the cursor resolves to the same country as its on-map copy.
pub fn region_at_point(
    geometry: &GeometryLayer,
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    surface_point: SurfacePoint,
) -> Option<RegionHit> {
    if surface_dimensions.width == 0 || surface_dimensions.height == 0 {
        return None;
    }

    let geo_point: GeoPoint = surface_to_geo(viewport, surface_dimensions, surface_point);
    let geo_point: GeoPoint = GeoPoint {
        lon: wrap_longitude(geo_point.lon),
        ..geo_point
    };
    let query_bbox: BoundingBox = BoundingBox::from_point(geo_point);

    let candidate_features: Vec<CountryFeature> = geometry.features_intersecting_bbox(query_bbox).ok()?;
    let hit_feature: &CountryFeature = candidate_features
        .iter()
        .find(|candidate_feature| candidate_feature.contains(geo_point))?;

    Some(RegionHit {
        region_code: RegionCode(hit_feature.region_code.clone()),
        iso3: hit_feature.iso3.clone(),
        name_en: hit_feature.name_en.clone(),
    })
}

/// Maps a device-pixel surface point through the viewport into Miller projected space. The gesture
/// helpers and `surface_to_geo` share this one normalization, so a pixel a gesture holds fixed keeps
/// resolving to the same projected point.
pub fn surface_to_projected(viewport: Viewport, surface_dimensions: SurfaceDimensions, surface_point: SurfacePoint) -> ProjectedPoint {
    let normalized_x: f64 = surface_point.x / surface_dimensions.width as f64;
    let normalized_y: f64 = surface_point.y / surface_dimensions.height as f64;

    // The normalized position places the cursor within the surface on [0, 1]: 0 at the left/top edge, 1
    // at the right/bottom. Surface y grows downward, so normalized_y 0 maps to the viewport's max
    // projected y (the top of the view), not its min.
    ProjectedPoint {
        x: viewport.min.x + normalized_x * (viewport.max.x - viewport.min.x),
        y: viewport.max.y - normalized_y * (viewport.max.y - viewport.min.y),
    }
}

fn surface_to_geo(viewport: Viewport, surface_dimensions: SurfaceDimensions, surface_point: SurfacePoint) -> GeoPoint {
    let projected: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, surface_point);

    projection::unproject(projected.x, projected.y)
}

/// The viewport after a drag-pan step: the world point under `from` tracks to `to`, then the view is kept
/// inside the home latitude range and re-normalized across the antimeridian.
pub fn pan(
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    from: SurfacePoint,
    to: SurfacePoint,
    home_min_y: f64,
    home_max_y: f64,
) -> Viewport {
    let from_projected: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, from);
    let to_projected: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, to);

    viewport
        .pan_by(from_projected.x - to_projected.x, from_projected.y - to_projected.y)
        .clamp_vertical(home_min_y, home_max_y)
        .normalize_longitude_turns()
}

/// The viewport after a wheel-zoom step: zoom by `factor` about the projected point under `surface_point`
/// (holding it under the cursor), then re-normalize across the antimeridian.
pub fn zoom_at_surface_point(
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    surface_point: SurfacePoint,
    factor: f64,
    max_half_height: f64,
    home_min_y: f64,
    home_max_y: f64,
) -> Viewport {
    let anchor: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, surface_point);

    viewport
        .zoom_about(factor, anchor, max_half_height, home_min_y, home_max_y, surface_dimensions)
        .normalize_longitude_turns()
}

/// The viewport after one incremental two-finger pinch step: scale by the ratio of the current to the
/// previous finger distance about the previous midpoint, then translate so that projected point tracks
/// the current midpoint (a similarity transform without rotation; the two contact points stay pinned
/// along the line between them). Kept inside the home latitude range and re-normalized. Coincident
/// previous fingers (a near-zero previous distance) leave the viewport unchanged.
pub fn pinch(
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    previous_a: SurfacePoint,
    previous_b: SurfacePoint,
    current_a: SurfacePoint,
    current_b: SurfacePoint,
    max_half_height: f64,
    home_min_y: f64,
    home_max_y: f64,
) -> Viewport {
    let previous_distance: f64 = surface_distance(previous_a, previous_b);
    if previous_distance <= f64::EPSILON {
        return viewport;
    }

    let factor: f64 = surface_distance(current_a, current_b) / previous_distance;
    let previous_midpoint: SurfacePoint = surface_midpoint(previous_a, previous_b);
    let current_midpoint: SurfacePoint = surface_midpoint(current_a, current_b);

    let anchor: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, previous_midpoint);
    let zoomed: Viewport = viewport.zoom_about(factor, anchor, max_half_height, home_min_y, home_max_y, surface_dimensions);

    // zoom_about held `anchor` under previous_midpoint; translate so it tracks to current_midpoint.
    let from: ProjectedPoint = surface_to_projected(zoomed, surface_dimensions, previous_midpoint);
    let to: ProjectedPoint = surface_to_projected(zoomed, surface_dimensions, current_midpoint);

    zoomed
        .pan_by(from.x - to.x, from.y - to.y)
        .clamp_vertical(home_min_y, home_max_y)
        .normalize_longitude_turns()
}

fn surface_midpoint(a: SurfacePoint, b: SurfacePoint) -> SurfacePoint {
    SurfacePoint { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 }
}

fn surface_distance(a: SurfacePoint, b: SurfacePoint) -> f64 {
    let dx: f64 = a.x - b.x;
    let dy: f64 = a.y - b.y;

    (dx * dx + dy * dy).sqrt()
}

fn wrap_longitude(lon: f64) -> f64 {
    (lon + 180.0).rem_euclid(360.0) - 180.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::geometry::parse_geometry_layer;
    use crate::artifact::geometry::tests::one_feature_fgb_bytes;

    const SURFACE_DIMENSIONS: SurfaceDimensions = SurfaceDimensions { width: 200, height: 200 };

    // Testland: a rectangle over lon 0..2, lat 0..3, region_code "testland".
    fn testland_geometry() -> GeometryLayer {
        parse_geometry_layer(one_feature_fgb_bytes()).unwrap()
    }

    // A viewport over longitude [min, max] and latitude 0..3, in projected space.
    fn latitude_band_viewport(longitude_min: f64, longitude_max: f64) -> Viewport {
        Viewport {
            min: projection::project(0.0, longitude_min),
            max: projection::project(3.0, longitude_max),
        }
    }

    fn testland_hit() -> RegionHit {
        RegionHit {
            region_code: RegionCode("testland".to_string()),
            iso3: "TST".to_string(),
            name_en: "Testland".to_string(),
        }
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn region_at_point_returns_the_country_under_the_cursor() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-10.0, 10.0);

        let result: Option<RegionHit> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, SurfacePoint { x: 110.0, y: 100.0 });

        assert_eq!(result, Some(testland_hit()));
    }

    #[test]
    fn region_at_point_is_scale_invariant_across_device_pixel_ratios() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-10.0, 10.0);

        // The driver scales both the canvas backing store and the pointer offset by devicePixelRatio, so
        // `region_at_point` must resolve the same region at any ratio (it normalizes the point against the
        // dimensions). A regression dropping that normalization is invisible at DPR 1.0, wrong on Retina.
        let base_dimensions: SurfaceDimensions = SurfaceDimensions { width: 200, height: 200 };
        let base_point: SurfacePoint = SurfacePoint { x: 110.0, y: 100.0 };

        for device_pixel_ratio in [1.0_f64, 2.0, 3.0] {
            let scaled_dimensions: SurfaceDimensions = SurfaceDimensions {
                width: (base_dimensions.width as f64 * device_pixel_ratio) as u32,
                height: (base_dimensions.height as f64 * device_pixel_ratio) as u32,
            };
            let scaled_point: SurfacePoint = SurfacePoint {
                x: base_point.x * device_pixel_ratio,
                y: base_point.y * device_pixel_ratio,
            };

            let result: Option<RegionHit> =
                region_at_point(&geometry, viewport, scaled_dimensions, scaled_point);

            assert_eq!(result, Some(testland_hit()), "DPR {device_pixel_ratio}");
        }
    }

    #[test]
    fn region_at_point_returns_none_over_open_ocean() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-10.0, 10.0);

        let result: Option<RegionHit> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, SurfacePoint { x: 50.0, y: 100.0 });

        assert_eq!(result, None);
    }

    #[test]
    fn region_at_point_wraps_longitude_past_the_antimeridian() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-370.0, -170.0);

        let result: Option<RegionHit> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, SurfacePoint { x: 11.0, y: 100.0 });

        assert_eq!(result, Some(testland_hit()));
    }

    const GESTURE_DIMENSIONS: SurfaceDimensions = SurfaceDimensions { width: 200, height: 100 };
    const GESTURE_HOME_MIN_Y: f64 = -2.0;
    const GESTURE_HOME_MAX_Y: f64 = 2.0;

    // Aspect 2 matches 200x100, so the viewport is undistorted and gestures stay well inside the range.
    fn gesture_viewport() -> Viewport {
        Viewport { min: ProjectedPoint { x: -2.0, y: -1.0 }, max: ProjectedPoint { x: 2.0, y: 1.0 } }
    }

    #[test]
    fn surface_to_projected_is_the_projected_half_of_surface_to_geo() {
        let viewport: Viewport = gesture_viewport();
        let point: SurfacePoint = SurfacePoint { x: 130.0, y: 40.0 };

        let projected: ProjectedPoint = surface_to_projected(viewport, GESTURE_DIMENSIONS, point);
        let geo: GeoPoint = surface_to_geo(viewport, GESTURE_DIMENSIONS, point);
        let expected: GeoPoint = projection::unproject(projected.x, projected.y);

        assert!((geo.lat - expected.lat).abs() < 1e-12);
        assert!((geo.lon - expected.lon).abs() < 1e-12);
    }

    #[test]
    fn pan_tracks_the_grabbed_point() {
        let viewport: Viewport = gesture_viewport();
        let from: SurfacePoint = SurfacePoint { x: 100.0, y: 50.0 };
        let to: SurfacePoint = SurfacePoint { x: 120.0, y: 40.0 };
        let grabbed: ProjectedPoint = surface_to_projected(viewport, GESTURE_DIMENSIONS, from);

        let panned: Viewport = pan(viewport, GESTURE_DIMENSIONS, from, to, GESTURE_HOME_MIN_Y, GESTURE_HOME_MAX_Y);
        let under_release: ProjectedPoint = surface_to_projected(panned, GESTURE_DIMENSIONS, to);

        assert!((under_release.x - grabbed.x).abs() < 1e-9, "grabbed point tracks to the release point (x)");
        assert!((under_release.y - grabbed.y).abs() < 1e-9, "grabbed point tracks to the release point (y)");
    }

    #[test]
    fn zoom_at_surface_point_holds_the_cursor() {
        let viewport: Viewport = gesture_viewport();
        let cursor: SurfacePoint = SurfacePoint { x: 150.0, y: 30.0 };
        let under_cursor: ProjectedPoint = surface_to_projected(viewport, GESTURE_DIMENSIONS, cursor);

        let zoomed: Viewport =
            zoom_at_surface_point(viewport, GESTURE_DIMENSIONS, cursor, 2.0, 4.0, GESTURE_HOME_MIN_Y, GESTURE_HOME_MAX_Y);
        let under_cursor_after: ProjectedPoint = surface_to_projected(zoomed, GESTURE_DIMENSIONS, cursor);

        assert!((under_cursor_after.x - under_cursor.x).abs() < 1e-9, "cursor x held");
        assert!((under_cursor_after.y - under_cursor.y).abs() < 1e-9, "cursor y held");
        assert!(((zoomed.max.y - zoomed.min.y) / 2.0 - 0.5).abs() < 1e-9, "half-height halved by factor 2");
    }

    #[test]
    fn pinch_zooms_about_the_fixed_midpoint() {
        let viewport: Viewport = gesture_viewport();
        // Fingers move apart symmetrically about (100, 50): distance 40 -> 80, factor 2 (zoom in).
        let previous_a: SurfacePoint = SurfacePoint { x: 80.0, y: 50.0 };
        let previous_b: SurfacePoint = SurfacePoint { x: 120.0, y: 50.0 };
        let current_a: SurfacePoint = SurfacePoint { x: 60.0, y: 50.0 };
        let current_b: SurfacePoint = SurfacePoint { x: 140.0, y: 50.0 };
        let midpoint: SurfacePoint = SurfacePoint { x: 100.0, y: 50.0 };
        let under_midpoint: ProjectedPoint = surface_to_projected(viewport, GESTURE_DIMENSIONS, midpoint);

        let pinched: Viewport = pinch(
            viewport, GESTURE_DIMENSIONS, previous_a, previous_b, current_a, current_b,
            4.0, GESTURE_HOME_MIN_Y, GESTURE_HOME_MAX_Y,
        );
        let under_midpoint_after: ProjectedPoint = surface_to_projected(pinched, GESTURE_DIMENSIONS, midpoint);

        assert!((under_midpoint_after.x - under_midpoint.x).abs() < 1e-9, "midpoint x pinned");
        assert!((under_midpoint_after.y - under_midpoint.y).abs() < 1e-9, "midpoint y pinned");
        assert!(((pinched.max.y - pinched.min.y) / 2.0 - 0.5).abs() < 1e-9, "half-height halved by the distance ratio");
    }

    #[test]
    fn pinch_translates_when_the_midpoint_moves() {
        let viewport: Viewport = gesture_viewport();
        // Both fingers shift right by 20 with distance unchanged (40): factor 1, midpoint 100 -> 120.
        let previous_a: SurfacePoint = SurfacePoint { x: 80.0, y: 50.0 };
        let previous_b: SurfacePoint = SurfacePoint { x: 120.0, y: 50.0 };
        let current_a: SurfacePoint = SurfacePoint { x: 100.0, y: 50.0 };
        let current_b: SurfacePoint = SurfacePoint { x: 140.0, y: 50.0 };
        let previous_midpoint: SurfacePoint = SurfacePoint { x: 100.0, y: 50.0 };
        let current_midpoint: SurfacePoint = SurfacePoint { x: 120.0, y: 50.0 };
        let under_previous_midpoint: ProjectedPoint = surface_to_projected(viewport, GESTURE_DIMENSIONS, previous_midpoint);

        let pinched: Viewport = pinch(
            viewport, GESTURE_DIMENSIONS, previous_a, previous_b, current_a, current_b,
            4.0, GESTURE_HOME_MIN_Y, GESTURE_HOME_MAX_Y,
        );
        let under_current_midpoint: ProjectedPoint = surface_to_projected(pinched, GESTURE_DIMENSIONS, current_midpoint);

        assert!((under_current_midpoint.x - under_previous_midpoint.x).abs() < 1e-9, "the point under the old midpoint tracks to the new midpoint (x)");
        assert!((under_current_midpoint.y - under_previous_midpoint.y).abs() < 1e-9, "and y");
        assert!(((pinched.max.y - pinched.min.y) / 2.0 - 1.0).abs() < 1e-9, "half-height unchanged at factor 1");
    }
}
