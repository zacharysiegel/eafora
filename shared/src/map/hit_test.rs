//! Surface point to region lookup: normalize the device-pixel cursor against the surface, map
//! it through the viewport into Miller projected space, inverse-project to a coordinate, then
//! point-in-polygon against the geometry layer's features.

use crate::artifact::geometry::{BoundingBox, CountryFeature, GeometryLayer};
use crate::map::projection::{self, GeoPoint};
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

fn surface_to_geo(viewport: Viewport, surface_dimensions: SurfaceDimensions, surface_point: SurfacePoint) -> GeoPoint {
    let normalized_x: f64 = surface_point.x / surface_dimensions.width as f64;
    let normalized_y: f64 = surface_point.y / surface_dimensions.height as f64;

    // normalized_x and normalized_y place the cursor within the surface on [0, 1]: 0 at the left/top
    // edge, 1 at the right/bottom. The viewport is already projected, so interpolate those positions
    // directly across its projected bounds. Surface y grows downward, so normalized_y 0 maps to the
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
}
