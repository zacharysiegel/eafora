//! Screen-space point to region lookup: normalize the device-pixel cursor against the surface, map
//! it through the viewport into Miller projected space, inverse-project to a coordinate, then
//! point-in-polygon against the geometry layer's features.

use crate::artifact::geometry::{BoundingBox, CountryFeature, GeometryLayer};
use crate::map::projection::{self, GeoPoint};
use crate::map::{RegionCode, ScreenPoint, SurfaceDimensions, Viewport};

/// A hit-test result: the region under the cursor plus the fields a caller needs (`iso3`, `name_en`),
/// resolved here so callers do not re-parse the geometry layer to recover them.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionHit {
    pub region_code: RegionCode,
    pub iso3: String,
    pub name_en: String,
}

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
) -> Option<RegionHit> {
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
        .find(|candidate_feature| candidate_feature.contains(geo_point))?;

    Some(RegionHit {
        region_code: RegionCode(hit_feature.region_code.clone()),
        iso3: hit_feature.iso3.clone(),
        name_en: hit_feature.name_en.clone(),
    })
}

fn screen_to_geo(viewport: Viewport, surface_dimensions: SurfaceDimensions, screen_point: ScreenPoint) -> GeoPoint {
    let normalized_x: f64 = screen_point.x / surface_dimensions.width as f64;
    let normalized_y: f64 = screen_point.y / surface_dimensions.height as f64;

    // normalized_x and normalized_y place the cursor within the surface on [0, 1]: 0 at the left/top
    // edge, 1 at the right/bottom. The viewport is already projected, so interpolate those positions
    // directly across its projected bounds. Screen y grows downward, so normalized_y 0 maps to the
    // viewport's max projected y (the top of the view), not its min.
    let projected_x: f64 = viewport.min.x + normalized_x * (viewport.max.x - viewport.min.x);
    let projected_y: f64 = viewport.max.y - normalized_y * (viewport.max.y - viewport.min.y);

    projection::unproject(projected_x, projected_y)
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
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, ScreenPoint { x: 110.0, y: 100.0 });

        assert_eq!(result, Some(testland_hit()));
    }

    #[test]
    fn region_at_point_returns_none_over_open_ocean() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-10.0, 10.0);

        let result: Option<RegionHit> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, ScreenPoint { x: 50.0, y: 100.0 });

        assert_eq!(result, None);
    }

    #[test]
    fn region_at_point_wraps_longitude_past_the_antimeridian() {
        let geometry: GeometryLayer = testland_geometry();
        let viewport: Viewport = latitude_band_viewport(-370.0, -170.0);

        let result: Option<RegionHit> =
            region_at_point(&geometry, viewport, SURFACE_DIMENSIONS, ScreenPoint { x: 11.0, y: 100.0 });

        assert_eq!(result, Some(testland_hit()));
    }
}
