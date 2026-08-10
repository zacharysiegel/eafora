//! Surface-to-viewport mapping: convert a device-pixel point through the viewport into Miller projected
//! space, resolve the region under it (inverse-project, then point-in-polygon against the geometry
//! layer), and apply pan, wheel-zoom, and two-finger pinch gestures to the viewport. The gesture math is
//! pure and shares one surface normalization with the hit-test.

use crate::artifact::geometry::{BoundingBox, CountryFeature, GeometryLayer, Polygon};
use crate::map::projection::{self, GeoPoint, ProjectedPoint};
use crate::map::{RegionCode, SurfacePoint, SurfaceDimensions, Viewport};

/// A country's projected framing for the zoom-to-country target: its bounding rectangle in projected
/// space and its area-weighted centroid. Longitudes are unwrapped into one contiguous frame before
/// projecting (see `country_framing`), so an antimeridian-crossing country frames its true extent rather
/// than the whole globe; the values may sit slightly past ±π in x, which the caller re-normalizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CountryFraming {
    pub min: ProjectedPoint,
    pub max: ProjectedPoint,
    pub centroid: ProjectedPoint,
}

/// A hit-test result: the region under the cursor plus the fields a caller needs (`iso3`, `name_en`) and
/// the country's projected framing for the zoom-to-country target, all resolved in the single hit-test
/// pass so callers do not re-query the geometry layer.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionHit {
    pub region_code: RegionCode,
    pub iso3: String,
    pub name_en: String,
    pub framing: CountryFraming,
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
        framing: country_framing(hit_feature),
    })
}

/// The projected bounding rectangle and area-weighted centroid of a country, computed after unwrapping
/// its vertex longitudes into a contiguous frame via a largest-longitude-gap cut, so an antimeridian-
/// crossing country (its Natural Earth geometry split near +180 and -180) frames its true extent rather
/// than the whole globe. The unwrap is a no-op for a non-crossing country. Assumes a country's true
/// longitude span is under 360° (every Natural Earth 50m country satisfies this).
pub fn country_framing(feature: &CountryFeature) -> CountryFraming {
    let base_lon: f64 = occupied_arc_start_longitude(feature);

    let mut min: ProjectedPoint = ProjectedPoint { x: f64::INFINITY, y: f64::INFINITY };
    let mut max: ProjectedPoint = ProjectedPoint { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY };
    for polygon in &feature.polygons {
        // Holes lie inside the exterior, so the exterior rings alone bound the country.
        for &(lon, lat) in &polygon.exterior {
            let projected: ProjectedPoint = projection::project(lat, unwrap_longitude(lon, base_lon));
            min.x = min.x.min(projected.x);
            min.y = min.y.min(projected.y);
            max.x = max.x.max(projected.x);
            max.y = max.y.max(projected.y);
        }
    }

    let bounds_center: ProjectedPoint = ProjectedPoint { x: (min.x + max.x) / 2.0, y: (min.y + max.y) / 2.0 };
    let centroid: ProjectedPoint = area_weighted_centroid(&feature.polygons, base_lon).unwrap_or(bounds_center);

    CountryFraming { min, max, centroid }
}

/// The longitude at the start of the country's occupied arc: the vertex just after the largest gap
/// between adjacent longitudes around the circle (including the wrap gap across ±180). Unwrapping every
/// longitude into `[base, base + 360)` then places the vertices contiguously, even across the seam.
fn occupied_arc_start_longitude(feature: &CountryFeature) -> f64 {
    let mut longitudes: Vec<f64> = feature.polygons
        .iter()
        .flat_map(|polygon| polygon.exterior.iter())
        .map(|&(lon, _lat)| lon)
        .collect();
    longitudes.sort_by(|a, b| a.partial_cmp(b).expect("longitudes are finite"));

    if longitudes.is_empty() {
        return 0.0;
    }

    // The wrap gap (from the max back up to the min) is the default largest gap, and its high side is the
    // minimum longitude; an internal gap wins when it is wider, and its high side is its upper endpoint.
    let mut base: f64 = longitudes[0];
    let mut largest_gap: f64 = (longitudes[0] + 360.0) - longitudes[longitudes.len() - 1];
    for window in longitudes.windows(2) {
        let gap: f64 = window[1] - window[0];
        if gap > largest_gap {
            largest_gap = gap;
            base = window[1];
        }
    }

    base
}

/// `lon` shifted by whole turns of 360° into `[base, base + 360)`.
fn unwrap_longitude(lon: f64, base: f64) -> f64 {
    base + (lon - base).rem_euclid(360.0)
}

/// The area-weighted centroid of the country's polygons in projected space (holes subtracted), or `None`
/// when the total area is negligible (a degenerate feature). Longitudes are unwrapped against `base_lon`
/// before projecting so the centroid lands on the landmass, not averaged across the antimeridian.
fn area_weighted_centroid(polygons: &[Polygon], base_lon: f64) -> Option<ProjectedPoint> {
    let mut weighted_x: f64 = 0.0;
    let mut weighted_y: f64 = 0.0;
    let mut total_area: f64 = 0.0;

    for polygon in polygons {
        let (exterior_area, exterior_centroid): (f64, ProjectedPoint) = ring_area_and_centroid(&polygon.exterior, base_lon);
        weighted_x += exterior_area * exterior_centroid.x;
        weighted_y += exterior_area * exterior_centroid.y;
        total_area += exterior_area;

        for interior in &polygon.interiors {
            let (hole_area, hole_centroid): (f64, ProjectedPoint) = ring_area_and_centroid(interior, base_lon);
            weighted_x -= hole_area * hole_centroid.x;
            weighted_y -= hole_area * hole_centroid.y;
            total_area -= hole_area;
        }
    }

    if total_area.abs() <= f64::EPSILON {
        return None;
    }

    Some(ProjectedPoint { x: weighted_x / total_area, y: weighted_y / total_area })
}

/// The unsigned area and centroid of a ring, its vertices unwrapped against `base_lon` then projected.
/// Winding-independent: reversing the ring flips both the signed area and the moment sums, so the
/// centroid is unchanged; the area is returned as a magnitude for the hole-subtracting sum above.
fn ring_area_and_centroid(ring: &[(f64, f64)], base_lon: f64) -> (f64, ProjectedPoint) {
    let projected: Vec<ProjectedPoint> = ring
        .iter()
        .map(|&(lon, lat)| projection::project(lat, unwrap_longitude(lon, base_lon)))
        .collect();

    let vertex_count: usize = projected.len();
    if vertex_count < 3 {
        return (0.0, ProjectedPoint { x: 0.0, y: 0.0 });
    }

    let mut signed_area_times_two: f64 = 0.0;
    let mut moment_x: f64 = 0.0;
    let mut moment_y: f64 = 0.0;
    for index in 0..vertex_count {
        let current: ProjectedPoint = projected[index];
        let next: ProjectedPoint = projected[(index + 1) % vertex_count];
        let cross: f64 = current.x * next.y - next.x * current.y;
        signed_area_times_two += cross;
        moment_x += (current.x + next.x) * cross;
        moment_y += (current.y + next.y) * cross;
    }

    if signed_area_times_two.abs() <= f64::EPSILON {
        return (0.0, ProjectedPoint { x: 0.0, y: 0.0 });
    }

    let centroid: ProjectedPoint = ProjectedPoint {
        x: moment_x / (3.0 * signed_area_times_two),
        y: moment_y / (3.0 * signed_area_times_two),
    };

    (signed_area_times_two.abs() / 2.0, centroid)
}

/// Maps a device-pixel surface point through the viewport into Miller projected space.
pub fn surface_to_projected(viewport: Viewport, surface_dimensions: SurfaceDimensions, surface_point: SurfacePoint) -> ProjectedPoint {
    let normalized_x: f64 = surface_point.x / surface_dimensions.width as f64;
    let normalized_y: f64 = surface_point.y / surface_dimensions.height as f64;

    // Surface y grows downward, so normalized_y 0 (the top edge) maps to the viewport's max projected y,
    // hence the `max.y -` term.
    ProjectedPoint {
        x: viewport.min.x + normalized_x * (viewport.max.x - viewport.min.x),
        y: viewport.max.y - normalized_y * (viewport.max.y - viewport.min.y),
    }
}

fn surface_to_geo(viewport: Viewport, surface_dimensions: SurfaceDimensions, surface_point: SurfacePoint) -> GeoPoint {
    let projected: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, surface_point);

    projection::unproject(projected.x, projected.y)
}

/// The viewport after a drag-pan step: the world point under `from` tracks to `to`.
pub fn pan(
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    from: SurfacePoint,
    to: SurfacePoint,
    min_y: f64,
    max_y: f64,
) -> Viewport {
    let from_projected: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, from);
    let to_projected: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, to);

    viewport
        .pan_by(from_projected.x - to_projected.x, from_projected.y - to_projected.y)
        .clamp_vertical(min_y, max_y)
        .normalize_longitude_turns()
}

/// The viewport after a wheel-zoom step: zoom by `factor` about the projected point under
/// `surface_point`, holding it under the cursor.
pub fn zoom_at_surface_point(
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    surface_point: SurfacePoint,
    factor: f64,
    max_height: f64,
    min_y: f64,
    max_y: f64,
) -> Viewport {
    let anchor: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, surface_point);

    viewport
        .zoom_about(factor, anchor, max_height, min_y, max_y, surface_dimensions)
        .normalize_longitude_turns()
}

/// The viewport after one incremental two-finger pinch step: scale by the ratio of the current to the
/// previous finger distance about the previous midpoint, then translate so that projected point tracks
/// the current midpoint (a similarity transform without rotation; the two contact points stay pinned
/// along the line between them). Coincident previous fingers (a near-zero previous distance) leave the
/// viewport unchanged.
pub fn pinch(
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    previous_a: SurfacePoint,
    previous_b: SurfacePoint,
    current_a: SurfacePoint,
    current_b: SurfacePoint,
    max_height: f64,
    min_y: f64,
    max_y: f64,
) -> Viewport {
    let previous_distance: f64 = previous_a.euclidean_distance(previous_b);
    if previous_distance <= f64::EPSILON {
        return viewport;
    }

    let factor: f64 = current_a.euclidean_distance(current_b) / previous_distance;
    let previous_midpoint: SurfacePoint = previous_a.midpoint(previous_b);
    let current_midpoint: SurfacePoint = current_a.midpoint(current_b);

    let anchor: ProjectedPoint = surface_to_projected(viewport, surface_dimensions, previous_midpoint);
    let zoomed: Viewport = viewport.zoom_about(factor, anchor, max_height, min_y, max_y, surface_dimensions);

    // zoom_about held `anchor` under previous_midpoint; translate so it tracks to current_midpoint.
    let from: ProjectedPoint = surface_to_projected(zoomed, surface_dimensions, previous_midpoint);
    let to: ProjectedPoint = surface_to_projected(zoomed, surface_dimensions, current_midpoint);

    zoomed
        .pan_by(from.x - to.x, from.y - to.y)
        .clamp_vertical(min_y, max_y)
        .normalize_longitude_turns()
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

    fn assert_is_testland(result: Option<RegionHit>) {
        let hit: RegionHit = result.expect("a region under the cursor");
        assert_eq!(hit.region_code, RegionCode("testland".to_string()));
        assert_eq!(hit.iso3, "TST");
        assert_eq!(hit.name_en, "Testland");
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn region_at_point_returns_the_country_under_the_cursor() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-10.0, 10.0);

        let result: Option<RegionHit> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, SurfacePoint { x: 110.0, y: 100.0 });

        assert_is_testland(result);
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

            assert_is_testland(result);
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

        assert_is_testland(result);
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

    fn framing_feature(polygons: Vec<Polygon>) -> CountryFeature {
        CountryFeature {
            iso3: "TST".to_string(),
            name_en: "Testland".to_string(),
            region_code: "testland".to_string(),
            polygons,
            bbox: BoundingBox { min_lon: 0.0, min_lat: 0.0, max_lon: 0.0, max_lat: 0.0 },
        }
    }

    #[test]
    fn country_framing_bounds_and_centers_a_simple_rectangle() {
        // A rectangle over lon 0..2, lat 0..3. Miller is separable, so the projected shape is an
        // axis-aligned rectangle and its area-weighted centroid is the geometric center.
        let feature: CountryFeature =
            framing_feature(vec![Polygon { exterior: vec![(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)], interiors: vec![] }]);
        let framing: CountryFraming = country_framing(&feature);

        assert!((framing.min.x - 0.0).abs() < 1e-12 && (framing.max.x - 2.0_f64.to_radians()).abs() < 1e-12, "x bounds are the projected lon extent");
        assert!((framing.min.y - 0.0).abs() < 1e-12 && (framing.max.y - projection::project(3.0, 0.0).y).abs() < 1e-12, "y bounds are the projected lat extent");
        assert!((framing.centroid.x - 1.0_f64.to_radians()).abs() < 1e-12, "centroid x is the rectangle center");
        assert!((framing.centroid.y - projection::project(3.0, 0.0).y / 2.0).abs() < 1e-12, "centroid y is the rectangle center");
    }

    #[test]
    fn country_framing_unwraps_an_antimeridian_country_to_its_true_extent() {
        // A quad straddling the seam, stored split near +179 and -179 (the Natural Earth shape). The
        // largest-gap unwrap must rejoin it as a ~2-degree strip near +180, not the whole globe.
        let feature: CountryFeature =
            framing_feature(vec![Polygon { exterior: vec![(179.0, 0.0), (-179.0, 0.0), (-179.0, 2.0), (179.0, 2.0)], interiors: vec![] }]);
        let framing: CountryFraming = country_framing(&feature);
        let width: f64 = framing.max.x - framing.min.x;

        assert!(width < 0.1, "framed to the true ~2-degree width, not the ~2π globe span");
        assert!((framing.centroid.x - 180.0_f64.to_radians()).abs() < 1e-6, "centroid on the date line, not averaged toward longitude 0");
    }

    #[test]
    fn country_framing_subtracts_a_hole_from_the_centroid() {
        // A square with a hole pushed to the right half pulls the centroid left of the square's center.
        let square: Vec<(f64, f64)> = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        let hole: Vec<(f64, f64)> = vec![(2.5, 1.0), (3.5, 1.0), (3.5, 3.0), (2.5, 3.0)];
        let feature: CountryFeature = framing_feature(vec![Polygon { exterior: square, interiors: vec![hole] }]);
        let framing: CountryFraming = country_framing(&feature);

        assert!(framing.centroid.x < 2.0_f64.to_radians(), "the hole on the right shifts the centroid left of the square center");
    }
}
