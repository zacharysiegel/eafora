//! Miller cylindrical projection — the only projection (see docs/architecture/overview.md §Projection).
//! Longitude is the x axis, passed through unchanged in degrees and NOT clamped to ±180 (the
//! renderer's horizontal-wraparound owns the antimeridian seam, not the projection); latitude
//! drives the cylindrical y. Pure functions: no I/O, no global state.

use std::f64::consts::FRAC_PI_4;

pub fn project(lon: f64, lat: f64) -> (f64, f64) {
    let lat_radians: f64 = lat.to_radians();
    let y: f64 = 1.25 * (FRAC_PI_4 + 0.4 * lat_radians).tan().ln();

    (lon, y)
}

pub fn unproject(x: f64, y: f64) -> (f64, f64) {
    let lat_radians: f64 = ((y / 1.25).exp().atan() - FRAC_PI_4) / 0.4;
    let lat: f64 = lat_radians.to_degrees();

    (x, lat)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f64 = 1e-10;

    #[test]
    fn project_maps_origin_to_origin() {
        let (x, y): (f64, f64) = project(0.0, 0.0);

        assert_eq!(x, 0.0);
        assert!(y.abs() < TOLERANCE);
    }

    #[test]
    fn project_passes_longitude_through_without_clamping() {
        let (x, y): (f64, f64) = project(-185.0, 0.0);

        assert_eq!(x, -185.0);
        assert!(y.abs() < TOLERANCE);
    }

    #[test]
    fn unproject_inverts_project_across_the_domain() {
        for latitude_degrees in -89..=89 {
            for longitude_degrees in -180..=180 {
                let lon: f64 = longitude_degrees as f64;
                let lat: f64 = latitude_degrees as f64;

                let (x, y): (f64, f64) = project(lon, lat);
                let (recovered_lon, recovered_lat): (f64, f64) = unproject(x, y);

                assert!((recovered_lon - lon).abs() < TOLERANCE, "lon {lon}: recovered {recovered_lon}");
                assert!((recovered_lat - lat).abs() < TOLERANCE, "lat {lat}: recovered {recovered_lat}");
            }
        }
    }
}
