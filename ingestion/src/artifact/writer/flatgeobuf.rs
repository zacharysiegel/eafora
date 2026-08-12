//! A Natural Earth feature is matched to a seeded country by its `ADM0_A3` code (translated to canonical
//! ISO3 first; see `natural_earth::canonical_iso3`). A code with no seeded country gets a warning logged
//! and the feature dropped: Natural Earth ships entries we intentionally omit, like Antarctica,
//! uninhabited islets, and the Siachen Glacier.
//!
//! Output is uncompressed. FlatGeobuf is already a packed binary format, but
//! a brotli pass over the finished `.fgb` still nets ~50-65% reduction
//! because coordinate sequences and the R-tree have residual structural
//! redundancy. The right place to add that is at publish time via HTTP
//! `Content-Encoding: br` (transparent to the browser, no client change);
//! not at write time, since the local artifact on disk is more useful as a
//! plain `.fgb` (loadable in QGIS, inspectable with `fgb info`). The
//! trade-off is that `Content-Encoding`-compressed bodies break FlatGeobuf's
//! HTTP-range-request streaming mode; v1 downloads the whole geometry shard
//! at startup so this doesn't bite. Worth revisiting if the 1:50m geometry
//! starts looking too coarse and we want to step up to 1:10m (approx. 5x
//! larger pre-compression).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};

use flatgeobuf::{ColumnType, FgbWriter, GeometryType};
use geozero::{ColumnValue, PropertyProcessor};
use shapefile::dbase::FieldValue;
use shapefile::{Reader, ShapeReader};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::artifact::artifact_db;
use crate::artifact::artifact_model::CountryMetadataProjection;
use shared::artifact::geometry;
use shared::artifact::manifest;
use shared::filesystem::FileReference;
use crate::error::AppError;
use crate::geometry::natural_earth::{self, ShapefileBytes};
use crate::http;

const ADM0_A3_FIELD: &str = "ADM0_A3";
pub const PLACEHOLDER_GEOMETRY_BYTES: &[u8] = b"FGB-PLACEHOLDER";
const COLUMN_ISO3: Column = Column { index: 0, name: geometry::FEATURE_COLUMN_ISO3 };
const COLUMN_NAME_EN: Column = Column { index: 1, name: geometry::FEATURE_COLUMN_NAME_EN };
const COLUMN_REGION_CODE: Column = Column { index: 2, name: geometry::FEATURE_COLUMN_REGION_CODE };

struct Column {
    index: usize,
    name: &'static str,
}

type ShapefileReader<'a> = Reader<Cursor<&'a [u8]>, Cursor<&'a [u8]>>;

pub async fn write_geometry<'e>(
    executor: impl PgExecutor<'e>,
    artifact_dir: &Path,
) -> Result<FileReference, AppError> {
    let shapefile_bytes: ShapefileBytes = natural_earth::download_pinned_release(&http::HTTP_CLIENT).await?;
    write_flatgeobuf_from_shapefile(executor, &shapefile_bytes, artifact_dir).await
}

pub async fn write_flatgeobuf_from_shapefile<'e>(
    executor: impl PgExecutor<'e>,
    shapefile_bytes: &ShapefileBytes,
    artifact_dir: &Path,
) -> Result<FileReference, AppError> {
    let iso3_to_metadata: BTreeMap<String, CountryMetadataProjection> =
        artifact_db::read_country_iso3_to_metadata(executor).await?;

    let path: PathBuf = build_tmp_geometry_path(artifact_dir)?;

    let mut writer: FgbWriter<'_> = FgbWriter::create(geometry::GEOMETRY_LAYER_NAME, GeometryType::MultiPolygon)?;
    writer.add_column(COLUMN_ISO3.name, ColumnType::String, |_fbb, _col| {});
    writer.add_column(COLUMN_NAME_EN.name, ColumnType::String, |_fbb, _col| {});
    writer.add_column(COLUMN_REGION_CODE.name, ColumnType::String, |_fbb, _col| {});

    let mut reader: Reader<Cursor<&[u8]>, Cursor<&[u8]>> = build_shapefile_reader(shapefile_bytes)?;

    // Group features by canonical ISO3 before emitting. Most countries contribute one feature, but the
    // two unrecognized territories Natural Earth ships as their own features (Somaliland, Northern
    // Cyprus) alias to their sovereign's ISO3, so they must merge into that country's feature rather than
    // producing a second feature sharing its region_code.
    let mut polygons_by_iso3: BTreeMap<String, Vec<geo_types::Polygon<f64>>> = BTreeMap::new();
    for shape_and_record in reader.iter_shapes_and_records() {
        let (shape, record) = shape_and_record?;

        let Some(adm0_a3) = read_character_field(&record, ADM0_A3_FIELD) else {
            continue;
        };
        let iso3: String = natural_earth::canonical_iso3(&adm0_a3).to_string();
        if !iso3_to_metadata.contains_key(&iso3) {
            log::warn!(
                "dropping Natural Earth feature with no seeded country; [adm0_a3={}]",
                adm0_a3,
            );
            continue;
        }

        let geometry: geo_types::Geometry<f64> = geo_types::Geometry::try_from(shape)?;
        polygons_by_iso3.entry(iso3).or_default().extend(polygons_of(geometry));
    }

    for (iso3, polygons) in &polygons_by_iso3 {
        let metadata: &CountryMetadataProjection = iso3_to_metadata.get(iso3)
            .expect("iso3 was validated present when grouping");
        let feature_geometry: geo_types::Geometry<f64> =
            geo_types::Geometry::MultiPolygon(geo_types::MultiPolygon(polygons.clone()));

        writer.add_feature_geom(feature_geometry, |feature| {
            feature.property(COLUMN_ISO3.index, COLUMN_ISO3.name, &ColumnValue::String(iso3)).ok();
            feature.property(COLUMN_NAME_EN.index, COLUMN_NAME_EN.name, &ColumnValue::String(&metadata.name_en)).ok();
            feature.property(COLUMN_REGION_CODE.index, COLUMN_REGION_CODE.name, &ColumnValue::String(&metadata.region_code)).ok();
        })?;
    }

    let file: File = File::create(&path)?;
    let mut file_writer: BufWriter<File> = BufWriter::new(file);
    writer.write(&mut file_writer)?;

    let byte_count: u64 = fs::metadata(&path)?.len();

    Ok(FileReference { path, byte_count })
}

pub fn write_placeholder_geometry(artifact_dir: &Path) -> Result<FileReference, AppError> {
    let path: PathBuf = build_tmp_geometry_path(artifact_dir)?;
    fs::write(&path, PLACEHOLDER_GEOMETRY_BYTES)?;

    Ok(FileReference {
        path,
        byte_count: PLACEHOLDER_GEOMETRY_BYTES.len() as u64,
    })
}

fn build_tmp_geometry_path(artifact_dir: &Path) -> Result<PathBuf, AppError> {
    let geometry_dir: PathBuf = artifact_dir.join(manifest::SUBDIR_GEOMETRY);
    fs::create_dir_all(&geometry_dir)?;
    let tmp_uuid: Uuid = Uuid::now_v7();
    Ok(geometry_dir.join(format!("{}.tmp-{}.{}", geometry::GEOMETRY_FILENAME_STEM, tmp_uuid, geometry::GEOMETRY_FILENAME_EXTENSION)))
}

fn build_shapefile_reader<'a>(
    shapefile_bytes: &'a ShapefileBytes,
) -> Result<ShapefileReader<'a>, AppError> {
    let shape_cursor: Cursor<&'a [u8]> = Cursor::new(shapefile_bytes.shp.as_slice());
    let shx_cursor: Cursor<&'a [u8]> = Cursor::new(shapefile_bytes.shx.as_slice());
    let dbf_cursor: Cursor<&'a [u8]> = Cursor::new(shapefile_bytes.dbf.as_slice());

    let shape_reader: ShapeReader<Cursor<&'a [u8]>> = ShapeReader::with_shx(shape_cursor, shx_cursor)?;
    let dbase_reader: shapefile::dbase::Reader<Cursor<&'a [u8]>> = shapefile::dbase::Reader::new(dbf_cursor)?;

    Ok(Reader::new(shape_reader, dbase_reader))
}

fn read_character_field(record: &shapefile::dbase::Record, field_name: &str) -> Option<String> {
    match record.get(field_name) {
        Some(FieldValue::Character(Some(value))) => Some(value.trim().to_string()),
        _ => None,
    }
}

/// The constituent polygons of a feature's geometry, so features grouped under one canonical ISO3 can
/// be concatenated into a single `MultiPolygon`. Admin-0 features are always polygonal; any other
/// geometry contributes nothing.
fn polygons_of(geometry: geo_types::Geometry<f64>) -> Vec<geo_types::Polygon<f64>> {
    match geometry {
        geo_types::Geometry::Polygon(polygon) => vec![polygon],
        geo_types::Geometry::MultiPolygon(multi_polygon) => multi_polygon.0,
        _ => Vec::new(),
    }
}
