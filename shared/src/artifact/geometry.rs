use std::io::Cursor;

use const_format::formatcp;
use flatgeobuf::{FallibleStreamingIterator, FeatureProperties, FgbFeature, FgbReader};
use geozero::ToGeo;

use crate::error::AppError;

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

pub const SHARD_FILENAME_EXTENSION: &str = "sqlite";
pub const GEOMETRY_FILENAME_EXTENSION: &str = "fgb";

#[derive(Debug, Clone)]
pub struct Feature {
    pub iso3: String,
    pub name_en: String,
    pub polygons: Vec<Polygon>,
    pub bbox: BoundingBox,
}

#[derive(Debug, Clone)]
pub struct Polygon {
    pub outer: Vec<(f64, f64)>,
    pub holes: Vec<Vec<(f64, f64)>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min_longitude: f64,
    pub min_latitude: f64,
    pub max_longitude: f64,
    pub max_latitude: f64,
}

/// Owns the geometry bytes and opens a fresh `FgbReader` per query. The upstream
/// `FgbReader::select_*` consume the reader (one pass each), so re-opening over
/// the owned bytes is how a single `FlatGeobufReader` serves repeated queries;
/// re-opening only re-reads the small header, and bbox queries still use the
/// file's R-tree index.
pub struct FlatGeobufReader {
    bytes: Vec<u8>,
}

impl FlatGeobufReader {
    /// Every feature in the file (consumed by 006-core-renderer for vertex upload).
    pub fn iter_features(&self) -> Result<Vec<Feature>, AppError> {
        let mut feature_iter = FgbReader::open(Cursor::new(self.bytes.as_slice()))?.select_all()?;

        let mut features: Vec<Feature> = Vec::new();
        while let Some(fgb_feature) = feature_iter.next()? {
            features.push(extract_feature(fgb_feature)?);
        }

        Ok(features)
    }

    /// Features whose bounding box intersects `bbox`, via the file's R-tree spatial
    /// index (consumed by 006-core-renderer's hit-test path).
    pub fn features_in_bbox(&self, bbox: BoundingBox) -> Result<Vec<Feature>, AppError> {
        let mut feature_iter = FgbReader::open(Cursor::new(self.bytes.as_slice()))?.select_bbox(
            bbox.min_longitude,
            bbox.min_latitude,
            bbox.max_longitude,
            bbox.max_latitude,
        )?;

        let mut features: Vec<Feature> = Vec::new();
        while let Some(fgb_feature) = feature_iter.next()? {
            features.push(extract_feature(fgb_feature)?);
        }

        Ok(features)
    }
}

/// Parse the geometry bytes eagerly; a parse failure (bad header) surfaces here
/// rather than on first query.
pub fn open_flatgeobuf_reader(bytes: Vec<u8>) -> Result<FlatGeobufReader, AppError> {
    FgbReader::open(Cursor::new(bytes.as_slice()))?;

    Ok(FlatGeobufReader { bytes })
}

fn extract_feature(fgb_feature: &FgbFeature) -> Result<Feature, AppError> {
    let iso3: String = fgb_feature.property(FEATURE_COLUMN_ISO3)?;
    let name_en: String = fgb_feature.property(FEATURE_COLUMN_NAME_EN)?;

    let geometry: geo_types::Geometry<f64> = fgb_feature.to_geo()?;
    let (polygons, bbox): (Vec<Polygon>, BoundingBox) = polygons_and_bounding_box(geometry)?;

    Ok(Feature { iso3, name_en, polygons, bbox })
}

fn polygons_and_bounding_box(geometry: geo_types::Geometry<f64>) -> Result<(Vec<Polygon>, BoundingBox), AppError> {
    let mut polygons: Vec<Polygon> = Vec::new();
    match geometry {
        geo_types::Geometry::Polygon(polygon) => polygons.push(convert_polygon(&polygon)),
        geo_types::Geometry::MultiPolygon(multi_polygon) => {
            for polygon in &multi_polygon {
                polygons.push(convert_polygon(polygon));
            }
        }
        other => {
            return Err(AppError::from(format!("expected (multi)polygon geometry, got {:?}", other)));
        }
    }

    let bounding_box: BoundingBox = compute_bounding_box(&polygons)
        .ok_or_else(|| AppError::from("geometry feature has no coordinates".to_string()))?;

    Ok((polygons, bounding_box))
}

fn convert_polygon(polygon: &geo_types::Polygon<f64>) -> Polygon {
    let outer: Vec<(f64, f64)> = polygon.exterior().coords().map(|coord| (coord.x, coord.y)).collect();
    let holes: Vec<Vec<(f64, f64)>> = polygon
        .interiors()
        .iter()
        .map(|ring| ring.coords().map(|coord| (coord.x, coord.y)).collect())
        .collect();

    Polygon { outer, holes }
}

fn compute_bounding_box(polygons: &[Polygon]) -> Option<BoundingBox> {
    let mut coordinates = polygons
        .iter()
        .flat_map(|polygon| polygon.outer.iter().chain(polygon.holes.iter().flatten()));

    let &(first_longitude, first_latitude): &(f64, f64) = coordinates.next()?;
    let mut bounding_box: BoundingBox = BoundingBox {
        min_longitude: first_longitude,
        min_latitude: first_latitude,
        max_longitude: first_longitude,
        max_latitude: first_latitude,
    };

    for &(longitude, latitude) in coordinates {
        bounding_box.min_longitude = bounding_box.min_longitude.min(longitude);
        bounding_box.min_latitude = bounding_box.min_latitude.min(latitude);
        bounding_box.max_longitude = bounding_box.max_longitude.max(longitude);
        bounding_box.max_latitude = bounding_box.max_latitude.max(latitude);
    }

    Some(bounding_box)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use flatgeobuf::{ColumnType, FgbWriter, GeometryType};
    use geozero::{ColumnValue, PropertyProcessor};

    /// Build a one-feature FlatGeobuf in memory (a rectangle for "TST" / "Testland")
    /// via the upstream writer, so the reader round-trip is tested without committing
    /// an opaque binary fixture. `pub(crate)` so `bundle.rs`'s tests reuse it.
    pub(crate) fn one_feature_fgb_bytes() -> Vec<u8> {
        let mut writer: FgbWriter<'_> = FgbWriter::create(GEOMETRY_LAYER_NAME, GeometryType::MultiPolygon).unwrap();
        writer.add_column(FEATURE_COLUMN_ISO3, ColumnType::String, |_fbb, _col| {});
        writer.add_column(FEATURE_COLUMN_NAME_EN, ColumnType::String, |_fbb, _col| {});

        let unit_square: geo_types::Geometry<f64> = geo_types::Geometry::MultiPolygon(geo_types::MultiPolygon(vec![
            geo_types::Polygon::new(
                geo_types::LineString::from(vec![(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0), (0.0, 0.0)]),
                vec![],
            ),
        ]));

        writer
            .add_feature_geom(unit_square, |feature| {
                feature.property(0, FEATURE_COLUMN_ISO3, &ColumnValue::String("TST")).unwrap();
                feature.property(1, FEATURE_COLUMN_NAME_EN, &ColumnValue::String("Testland")).unwrap();
            })
            .unwrap();

        let mut buffer: Vec<u8> = Vec::new();
        writer.write(&mut buffer).unwrap();

        buffer
    }

    #[test]
    fn open_flatgeobuf_reader_parses_known_fixture() {
        let reader: FlatGeobufReader = open_flatgeobuf_reader(one_feature_fgb_bytes()).unwrap();

        let features: Vec<Feature> = reader.iter_features().unwrap();

        assert_eq!(features.len(), 1);
        let feature: &Feature = &features[0];
        assert_eq!(feature.iso3, "TST");
        assert_eq!(feature.name_en, "Testland");
        assert_eq!(feature.polygons.len(), 1);
        assert_eq!(feature.bbox, BoundingBox { min_longitude: 0.0, min_latitude: 0.0, max_longitude: 2.0, max_latitude: 3.0 });
    }

    #[test]
    fn features_in_bbox_returns_intersecting_feature() {
        let reader: FlatGeobufReader = open_flatgeobuf_reader(one_feature_fgb_bytes()).unwrap();

        let hits: Vec<Feature> = reader
            .features_in_bbox(BoundingBox { min_longitude: 0.5, min_latitude: 0.5, max_longitude: 1.0, max_latitude: 1.0 })
            .unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].iso3, "TST");
    }

    #[test]
    fn open_flatgeobuf_reader_rejects_garbage_bytes() {
        let result: Result<FlatGeobufReader, AppError> = open_flatgeobuf_reader(b"not a flatgeobuf".to_vec());

        assert!(result.is_err());
    }
}
