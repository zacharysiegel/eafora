//! Miller cylindrical projection. Longitude is the x axis, passed through unchanged in degrees and
//! NOT clamped to ±180 (the renderer's horizontal-wraparound owns the antimeridian seam, not the
//! projection); latitude drives the cylindrical y.

use std::f64::consts::FRAC_PI_4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedPoint {
    pub x: f64,
    pub y: f64,
}

pub fn project(lat: f64, lon: f64) -> ProjectedPoint {
    let lat_radians: f64 = lat.to_radians();
    let y: f64 = 1.25 * (FRAC_PI_4 + 0.4 * lat_radians).tan().ln();

    ProjectedPoint { x: lon, y }
}

pub fn unproject(x: f64, y: f64) -> GeoPoint {
    let lat_radians: f64 = ((y / 1.25).exp().atan() - FRAC_PI_4) / 0.4;
    let lat: f64 = lat_radians.to_degrees();

    GeoPoint { lat, lon: x }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f64 = 1e-10;

    #[test]
    fn project_maps_origin_to_origin() {
        let projected: ProjectedPoint = project(0.0, 0.0);

        assert_eq!(projected.x, 0.0);
        assert!(projected.y.abs() < TOLERANCE);
    }

    #[test]
    fn project_passes_longitude_through_without_clamping() {
        let projected: ProjectedPoint = project(0.0, -185.0);

        assert_eq!(projected.x, -185.0);
        assert!(projected.y.abs() < TOLERANCE);
    }

    #[test]
    fn unproject_inverts_project_across_the_domain() {
        for latitude_degrees in -89..=89 {
            for longitude_degrees in -180..=180 {
                let lon: f64 = longitude_degrees as f64;
                let lat: f64 = latitude_degrees as f64;

                let projected: ProjectedPoint = project(lat, lon);
                let recovered: GeoPoint = unproject(projected.x, projected.y);

                assert!((recovered.lon - lon).abs() < TOLERANCE, "lon {lon}: recovered {}", recovered.lon);
                assert!((recovered.lat - lat).abs() < TOLERANCE, "lat {lat}: recovered {}", recovered.lat);
            }
        }
    }
}
