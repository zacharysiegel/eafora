use std::io::Cursor;

use const_format::formatcp;
use flatgeobuf::{FallibleStreamingIterator, FeatureProperties, FgbFeature, FgbReader};
use geozero::ToGeo;

use crate::error::AppError;
use crate::GeoPoint;

/// Natural Earth scale denominator (1:50m). The single source for the scale token
/// shared by the layer name and the filename stem; a bump to 1:10m geometry changes
/// only this.
const GEOMETRY_SCALE: &str = "50m";

/// FlatGeobuf layer name baked into the `.fgb` (the producer writes it; readers / QGIS see it).
pub const GEOMETRY_LAYER_NAME: &str = formatcp!("world_{}_admin_0", GEOMETRY_SCALE);

/// Filename stem the producer uses; final filename is `{stem}-{sha8}.fgb`.
pub const GEOMETRY_FILENAME_STEM: &str = formatcp!("world-{}", GEOMETRY_SCALE);

/// FlatGeobuf feature column carrying the country's ISO 3166 alpha-3 code.
pub const FEATURE_COLUMN_ISO3: &str = "iso3";

/// FlatGeobuf feature column carrying the country's English name.
pub const FEATURE_COLUMN_NAME_EN: &str = "name_en";

/// FlatGeobuf feature column carrying the `region.code` slug of the region the country belongs to.
pub const FEATURE_COLUMN_REGION_CODE: &str = "region_code";

pub const SHARD_FILENAME_EXTENSION: &str = "sqlite";
pub const GEOMETRY_FILENAME_EXTENSION: &str = "fgb";

#[derive(Debug, Clone)]
pub struct Polygon {
    pub exterior: Vec<(f64, f64)>,
    pub interiors: Vec<Vec<(f64, f64)>>,
}

impl From<&geo_types::Polygon<f64>> for Polygon {
    fn from(polygon: &geo_types::Polygon<f64>) -> Self {
        let exterior: Vec<(f64, f64)> = polygon.exterior().coords().map(|coord| (coord.x, coord.y)).collect();
        let interiors: Vec<Vec<(f64, f64)>> = polygon
            .interiors()
            .iter()
            .map(|ring| ring.coords().map(|coord| (coord.x, coord.y)).collect())
            .collect();

        Polygon { exterior, interiors }
    }
}

impl Polygon {
    pub fn contains(&self, point: GeoPoint) -> bool {
        if !Self::point_in_ring(&self.exterior, point) {
            return false;
        }

        let inside_a_hole: bool = self
            .interiors
            .iter()
            .any(|interior_ring| Self::point_in_ring(interior_ring, point));

        !inside_a_hole
    }

    /// Even-odd rule: `point` is inside the ring when a ray cast east from it crosses an odd number
    /// of the ring's edges.
    fn point_in_ring(ring: &[(f64, f64)], point: GeoPoint) -> bool {
        let vertex_count: usize = ring.len();
        if vertex_count < 3 {
            return false;
        }

        let mut inside: bool = false;
        let mut previous_index: usize = vertex_count - 1;
        for current_index in 0..vertex_count {
            let edge_start: (f64, f64) = ring[previous_index];
            let edge_end: (f64, f64) = ring[current_index];

            if Self::edge_crosses_eastward_ray(edge_start, edge_end, point) {
                inside = !inside;
            }

            previous_index = current_index;
        }

        inside
    }

    /// Whether the edge from `a` to `b` crosses a ray cast east from `point` (latitude held at
    /// `point.lat`, longitude increasing). True only when the endpoints sit on opposite sides of
    /// `point.lat` and the edge meets that latitude at a longitude east of `point.lon`.
    fn edge_crosses_eastward_ray(a: (f64, f64), b: (f64, f64), point: GeoPoint) -> bool {
        let (a_lon, a_lat): (f64, f64) = a;
        let (b_lon, b_lat): (f64, f64) = b;

        let a_is_above: bool = a_lat > point.lat;
        let b_is_above: bool = b_lat > point.lat;
        if a_is_above == b_is_above {
            return false;
        }

        let t: f64 = inverse_lerp(b_lat, a_lat, point.lat);
        let crossing_lon: f64 = lerp(b_lon, a_lon, t);

        crossing_lon > point.lon
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl BoundingBox {
    pub fn from_point(geo_point: GeoPoint) -> Self {
        BoundingBox {
            min_lon: geo_point.lon,
            min_lat: geo_point.lat,
            max_lon: geo_point.lon,
            max_lat: geo_point.lat,
        }
    }

    fn from_polygons(polygons: &[Polygon]) -> Option<Self> {
        let mut coordinates = polygons
            .iter()
            .flat_map(|polygon| polygon.exterior.iter().chain(polygon.interiors.iter().flatten()));

        let &(first_lon, first_lat): &(f64, f64) = coordinates.next()?;
        let mut bounding_box: BoundingBox = BoundingBox {
            min_lon: first_lon,
            min_lat: first_lat,
            max_lon: first_lon,
            max_lat: first_lat,
        };

        for &(lon, lat) in coordinates {
            bounding_box.min_lon = bounding_box.min_lon.min(lon);
            bounding_box.min_lat = bounding_box.min_lat.min(lat);
            bounding_box.max_lon = bounding_box.max_lon.max(lon);
            bounding_box.max_lat = bounding_box.max_lat.max(lat);
        }

        Some(bounding_box)
    }
}

#[derive(Debug, Clone)]
pub struct CountryFeature {
    pub iso3: String,
    pub name_en: String,
    pub region_code: String,
    pub polygons: Vec<Polygon>,
    pub bbox: BoundingBox,
}

impl<'a> TryFrom<&'a FgbFeature> for CountryFeature {
    type Error = AppError;

    fn try_from(fgb_feature: &'a FgbFeature) -> Result<Self, AppError> {
        let iso3: String = fgb_feature.property(FEATURE_COLUMN_ISO3)?;
        let name_en: String = fgb_feature.property(FEATURE_COLUMN_NAME_EN)?;
        let region_code: String = fgb_feature.property(FEATURE_COLUMN_REGION_CODE)?;

        let geometry: geo_types::Geometry<f64> = fgb_feature.to_geo()?;
        let polygons: Vec<Polygon> = polygons_from_geometry(geometry)?;

        let bbox: BoundingBox = BoundingBox::from_polygons(&polygons)
            .ok_or_else(|| AppError::from("geometry feature has no coordinates".to_string()))?;

        Ok(CountryFeature { iso3, name_en, region_code, polygons, bbox })
    }
}

impl CountryFeature {
    pub fn contains(&self, point: GeoPoint) -> bool {
        self.polygons.iter().any(|polygon| polygon.contains(point))
    }
}

/// Owns the geometry bytes and opens a fresh `FgbReader` per query. The upstream
/// `FgbReader::select_*` consume the reader (one pass each), so re-opening over
/// the owned bytes is how a single `GeometryLayer` serves repeated queries;
/// re-opening only re-reads the small header, and bbox queries still use the
/// file's R-tree index.
pub struct GeometryLayer {
    bytes: Vec<u8>,
}

impl GeometryLayer {
    /// All features in the file, collected eagerly.
    pub fn iter_features(&self) -> Result<Vec<CountryFeature>, AppError> {
        let mut feature_iter = FgbReader::open(Cursor::new(self.bytes.as_slice()))?.select_all()?;

        let mut country_features: Vec<CountryFeature> = Vec::new();
        while let Some(fgb_feature) = feature_iter.next()? {
            country_features.push(CountryFeature::try_from(fgb_feature)?);
        }

        Ok(country_features)
    }

    /// Features whose bounding box intersects `bbox`, via the file's R-tree spatial index.
    pub fn features_intersecting_bbox(&self, bbox: BoundingBox) -> Result<Vec<CountryFeature>, AppError> {
        let mut feature_iter = FgbReader::open(Cursor::new(self.bytes.as_slice()))?.select_bbox(
            bbox.min_lon,
            bbox.min_lat,
            bbox.max_lon,
            bbox.max_lat,
        )?;

        let mut country_features: Vec<CountryFeature> = Vec::new();
        while let Some(fgb_feature) = feature_iter.next()? {
            country_features.push(CountryFeature::try_from(fgb_feature)?);
        }

        Ok(country_features)
    }
}

/// Parse the geometry bytes eagerly; a parse failure (bad header) surfaces here
/// rather than on first query.
pub fn parse_geometry_layer(bytes: Vec<u8>) -> Result<GeometryLayer, AppError> {
    FgbReader::open(Cursor::new(bytes.as_slice()))?;

    Ok(GeometryLayer { bytes })
}

fn polygons_from_geometry(geometry: geo_types::Geometry<f64>) -> Result<Vec<Polygon>, AppError> {
    let mut polygons: Vec<Polygon> = Vec::new();
    match geometry {
        geo_types::Geometry::Polygon(polygon) => polygons.push(Polygon::from(&polygon)),
        geo_types::Geometry::MultiPolygon(multi_polygon) => {
            for polygon in &multi_polygon {
                polygons.push(Polygon::from(polygon));
            }
        }
        other => {
            return Err(AppError::from(format!("expected (multi)polygon geometry, got {:?}", other)));
        }
    }

    Ok(polygons)
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + t * (to - from)
}

fn inverse_lerp(from: f64, to: f64, value: f64) -> f64 {
    (value - from) / (to - from)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// One feature: a rectangle over lon 0..2, lat 0..3, iso3 "TST" / name_en "Testland" / region_code "testland".
    pub(crate) fn one_feature_fgb_bytes() -> Vec<u8> {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/one-feature.fgb")).to_vec()
    }

    /// Regenerates the committed one-feature sample. There is no producer path for a synthetic
    /// feature, so the writer is exercised here directly; run after changing the feature columns.
    #[test]
    #[ignore = "run manually to regenerate tests/samples/one-feature.fgb"]
    #[cfg(not(target_arch = "wasm32"))] // not for wasm32: writes the committed sample via std::fs
    fn dump_one_feature_fgb() {
        use flatgeobuf::{ColumnType, FgbWriter, GeometryType};
        use geozero::{ColumnValue, PropertyProcessor};

        let mut writer: FgbWriter<'_> = FgbWriter::create(GEOMETRY_LAYER_NAME, GeometryType::MultiPolygon).unwrap();
        writer.add_column(FEATURE_COLUMN_ISO3, ColumnType::String, |_fbb, _col| {});
        writer.add_column(FEATURE_COLUMN_NAME_EN, ColumnType::String, |_fbb, _col| {});
        writer.add_column(FEATURE_COLUMN_REGION_CODE, ColumnType::String, |_fbb, _col| {});

        let rectangle: geo_types::Polygon<f64> = geo_types::Polygon::new(
            geo_types::LineString::from(vec![(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0), (0.0, 0.0)]),
            vec![],
        );
        let geometry: geo_types::Geometry<f64> =
            geo_types::Geometry::MultiPolygon(geo_types::MultiPolygon(vec![rectangle]));

        writer
            .add_feature_geom(geometry, |feature| {
                feature.property(0, FEATURE_COLUMN_ISO3, &ColumnValue::String("TST")).ok();
                feature.property(1, FEATURE_COLUMN_NAME_EN, &ColumnValue::String("Testland")).ok();
                feature.property(2, FEATURE_COLUMN_REGION_CODE, &ColumnValue::String("testland")).ok();
            })
            .unwrap();

        let mut bytes: Vec<u8> = Vec::new();
        writer.write(&mut bytes).unwrap();
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/one-feature.fgb"), bytes).unwrap();
    }

    // flatgeobuf/geozero can compile for wasm yet trap at runtime (e.g. filesystem access), so a
    // green `cargo check --target wasm32` doesn't prove the reader runs there; this checks it does.
    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn parse_geometry_layer_parses_known_fixture() {
        let geometry_layer: GeometryLayer = parse_geometry_layer(one_feature_fgb_bytes()).unwrap();

        let country_features: Vec<CountryFeature> = geometry_layer.iter_features().unwrap();

        assert_eq!(country_features.len(), 1);
        let country_feature: &CountryFeature = &country_features[0];
        assert_eq!(country_feature.iso3, "TST");
        assert_eq!(country_feature.name_en, "Testland");
        assert_eq!(country_feature.region_code, "testland");
        assert_eq!(country_feature.polygons.len(), 1);
        assert_eq!(country_feature.bbox, BoundingBox { min_lon: 0.0, min_lat: 0.0, max_lon: 2.0, max_lat: 3.0 });
    }

    #[test]
    fn features_intersecting_bbox_returns_matching_feature() {
        let geometry_layer: GeometryLayer = parse_geometry_layer(one_feature_fgb_bytes()).unwrap();

        let country_feature_hits: Vec<CountryFeature> = geometry_layer
            .features_intersecting_bbox(BoundingBox { min_lon: 0.5, min_lat: 0.5, max_lon: 1.0, max_lat: 1.0 })
            .unwrap();

        assert_eq!(country_feature_hits.len(), 1);
        assert_eq!(country_feature_hits[0].iso3, "TST");
    }

    #[test]
    fn parse_geometry_layer_rejects_garbage_bytes() {
        let result: Result<GeometryLayer, AppError> = parse_geometry_layer(b"not a flatgeobuf".to_vec());

        assert!(result.is_err());
    }
}
