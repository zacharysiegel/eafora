//! Screen-space point to region lookup: normalize the device-pixel cursor against the surface, map
//! it through the viewport into Miller projected space, inverse-project to a coordinate, then
//! point-in-polygon against the geometry layer's features.

use crate::artifact::geometry::{BoundingBox, CountryFeature, GeometryLayer, Polygon};
use crate::map::projection::{self, GeoPoint};
use crate::map::value_types::{RegionCode, ScreenPoint, SurfaceDimensions, Viewport};

/// The region whose polygon contains `screen_point`, or `None` when the point is off every country
/// (open ocean) or off the map. `surface_dimensions` is required because `screen_point` is in device
/// pixels: the point is normalized against the surface extent before it can be mapped through the
/// viewport. A longitude past the ±180 seam (from a pan across the antimeridian) is wrapped back
/// into range so the cursor resolves to the same country as its on-map copy.
pub fn region_at_point(
    geometry: &GeometryLayer,
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    screen_point: ScreenPoint,
) -> Option<RegionCode> {
    if surface_dimensions.width == 0 || surface_dimensions.height == 0 {
        return None;
    }

    let geo_point: GeoPoint = screen_to_geo(viewport, surface_dimensions, screen_point);
    let geo_point: GeoPoint = GeoPoint {
        lon: wrap_longitude(geo_point.lon),
        ..geo_point
    };
    let query_bbox: BoundingBox = BoundingBox::from_point(geo_point);

    let candidate_features: Vec<CountryFeature> = geometry.features_intersecting_bbox(query_bbox).ok()?;
    let hit_feature: &CountryFeature = candidate_features
        .iter()
        .find(|candidate_feature| feature_contains(candidate_feature, geo_point.lat, geo_point.lon))?;

    Some(RegionCode(hit_feature.region_code.clone()))
}

fn screen_to_geo(viewport: Viewport, surface_dimensions: SurfaceDimensions, screen_point: ScreenPoint) -> GeoPoint {
    let horizontal_fraction: f64 = screen_point.x / surface_dimensions.width as f64;
    let vertical_fraction: f64 = screen_point.y / surface_dimensions.height as f64;

    let projected_x: f64 =
        viewport.longitude_min + horizontal_fraction * (viewport.longitude_max - viewport.longitude_min);

    let projected_y_top: f64 = projection::project(0.0, viewport.latitude_max).y;
    let projected_y_bottom: f64 = projection::project(0.0, viewport.latitude_min).y;
    let projected_y: f64 = projected_y_top + vertical_fraction * (projected_y_bottom - projected_y_top);

    projection::unproject(projected_x, projected_y)
}

fn wrap_longitude(lon: f64) -> f64 {
    (lon + 180.0).rem_euclid(360.0) - 180.0
}

fn feature_contains(feature: &CountryFeature, lat: f64, lon: f64) -> bool {
    feature.polygons.iter().any(|polygon| point_in_polygon(polygon, lat, lon))
}

fn point_in_polygon(polygon: &Polygon, lat: f64, lon: f64) -> bool {
    if !point_in_ring(&polygon.exterior, lat, lon) {
        return false;
    }

    let inside_a_hole: bool = polygon
        .interiors
        .iter()
        .any(|interior_ring| point_in_ring(interior_ring, lat, lon));

    !inside_a_hole
}

/// Even-odd ray casting: a point is inside when a ray cast to +longitude crosses an odd number of
/// ring edges.
fn point_in_ring(ring: &[(f64, f64)], lat: f64, lon: f64) -> bool {
    let vertex_count: usize = ring.len();
    if vertex_count < 3 {
        return false;
    }

    let mut inside: bool = false;
    let mut previous_index: usize = vertex_count - 1;
    for current_index in 0..vertex_count {
        let (current_lon, current_lat): (f64, f64) = ring[current_index];
        let (previous_lon, previous_lat): (f64, f64) = ring[previous_index];

        let edge_straddles_latitude: bool = (current_lat > lat) != (previous_lat > lat);
        if edge_straddles_latitude {
            let crossing_lon: f64 =
                (previous_lon - current_lon) * (lat - current_lat) / (previous_lat - current_lat) + current_lon;

            if lon < crossing_lon {
                inside = !inside;
            }
        }

        previous_index = current_index;
    }

    inside
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

    fn latitude_band_viewport(longitude_min: f64, longitude_max: f64) -> Viewport {
        Viewport {
            longitude_min,
            longitude_max,
            latitude_min: 0.0,
            latitude_max: 3.0,
        }
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn region_at_point_returns_the_country_under_the_cursor() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-10.0, 10.0);

        let result: Option<RegionCode> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, ScreenPoint { x: 110.0, y: 100.0 });

        assert_eq!(result, Some(RegionCode("testland".to_string())));
    }

    #[test]
    fn region_at_point_returns_none_over_open_ocean() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-10.0, 10.0);

        let result: Option<RegionCode> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, ScreenPoint { x: 50.0, y: 100.0 });

        assert_eq!(result, None);
    }

    #[test]
    fn region_at_point_wraps_longitude_past_the_antimeridian() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-370.0, -170.0);

        let result: Option<RegionCode> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, ScreenPoint { x: 11.0, y: 100.0 });

        assert_eq!(result, Some(RegionCode("testland".to_string())));
    }
}
