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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl BoundingBox {
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
    pub polygons: Vec<Polygon>,
    pub bbox: BoundingBox,
}

impl<'a> TryFrom<&'a FgbFeature> for CountryFeature {
    type Error = AppError;

    fn try_from(fgb_feature: &'a FgbFeature) -> Result<Self, AppError> {
        let iso3: String = fgb_feature.property(FEATURE_COLUMN_ISO3)?;
        let name_en: String = fgb_feature.property(FEATURE_COLUMN_NAME_EN)?;

        let geometry: geo_types::Geometry<f64> = fgb_feature.to_geo()?;
        let polygons: Vec<Polygon> = polygons_from_geometry(geometry)?;

        let bbox: BoundingBox = BoundingBox::from_polygons(&polygons)
            .ok_or_else(|| AppError::from("geometry feature has no coordinates".to_string()))?;

        Ok(CountryFeature { iso3, name_en, polygons, bbox })
    }
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
    pub fn features_in_bbox(&self, bbox: BoundingBox) -> Result<Vec<CountryFeature>, AppError> {
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
pub fn open_flatgeobuf_reader(bytes: Vec<u8>) -> Result<FlatGeobufReader, AppError> {
    FgbReader::open(Cursor::new(bytes.as_slice()))?;

    Ok(FlatGeobufReader { bytes })
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

        let country_features: Vec<CountryFeature> = reader.iter_features().unwrap();

        assert_eq!(country_features.len(), 1);
        let country_feature: &CountryFeature = &country_features[0];
        assert_eq!(country_feature.iso3, "TST");
        assert_eq!(country_feature.name_en, "Testland");
        assert_eq!(country_feature.polygons.len(), 1);
        assert_eq!(country_feature.bbox, BoundingBox { min_lon: 0.0, min_lat: 0.0, max_lon: 2.0, max_lat: 3.0 });
    }

    #[test]
    fn features_in_bbox_returns_intersecting_feature() {
        let reader: FlatGeobufReader = open_flatgeobuf_reader(one_feature_fgb_bytes()).unwrap();

        let country_feature_hits: Vec<CountryFeature> = reader
            .features_in_bbox(BoundingBox { min_lon: 0.5, min_lat: 0.5, max_lon: 1.0, max_lat: 1.0 })
            .unwrap();

        assert_eq!(country_feature_hits.len(), 1);
        assert_eq!(country_feature_hits[0].iso3, "TST");
    }

    #[test]
    fn open_flatgeobuf_reader_rejects_garbage_bytes() {
        let result: Result<FlatGeobufReader, AppError> = open_flatgeobuf_reader(b"not a flatgeobuf".to_vec());

        assert!(result.is_err());
    }
}
