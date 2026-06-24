//! Unknown ADM0_A3 codes get a warning logged and the feature dropped.
//! Natural Earth ships entries like `KOS` (Kosovo) that some downstream
//! consumers don't recognize.
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
use shared::artifact::manifest;
use shared::filesystem::FileReference;
use crate::error::AppError;
use crate::geometry::natural_earth::{self, ShapefileBytes};
use crate::http;

pub const GEOMETRY_LAYER_NAME: &str = "world_50m_admin_0";
pub const GEOMETRY_FILENAME_STEM: &str = "world-50m";
const ADM0_A3_FIELD: &str = "ADM0_A3";
pub const PLACEHOLDER_GEOMETRY_BYTES: &[u8] = b"FGB-PLACEHOLDER";
const COLUMN_ISO3: Column = Column { index: 0, name: "iso3" };
const COLUMN_NAME_EN: Column = Column { index: 1, name: "name_en" };

struct Column {
    index: usize,
    name: &'static str,
}

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
    let iso3_to_name_en: BTreeMap<String, String> =
        artifact_db::read_country_iso3_to_name_en(executor).await?;

    let path: PathBuf = build_tmp_geometry_path(artifact_dir)?;

    let mut writer: FgbWriter<'_> = FgbWriter::create(GEOMETRY_LAYER_NAME, GeometryType::MultiPolygon)?;
    writer.add_column(COLUMN_ISO3.name, ColumnType::String, |_fbb, _col| {});
    writer.add_column(COLUMN_NAME_EN.name, ColumnType::String, |_fbb, _col| {});

    let mut reader: Reader<Cursor<&[u8]>, Cursor<&[u8]>> = build_shapefile_reader(shapefile_bytes)?;

    for shape_and_record in reader.iter_shapes_and_records() {
        let (shape, record) = shape_and_record?;

        let Some(iso3) = read_character_field(&record, ADM0_A3_FIELD) else {
            continue;
        };
        let Some(name_en) = iso3_to_name_en.get(&iso3) else {
            log::warn!(
                "dropping Natural Earth feature with unknown ADM0_A3={}",
                iso3,
            );
            continue;
        };

        let geometry: geo_types::Geometry<f64> = geo_types::Geometry::try_from(shape)?;
        let iso3_property: String = iso3.clone();
        let name_en_property: String = name_en.clone();

        writer.add_feature_geom(geometry, |feature| {
            feature.property(COLUMN_ISO3.index, COLUMN_ISO3.name, &ColumnValue::String(&iso3_property)).ok();
            feature.property(COLUMN_NAME_EN.index, COLUMN_NAME_EN.name, &ColumnValue::String(&name_en_property)).ok();
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
    Ok(geometry_dir.join(format!("{}.tmp-{}.fgb", GEOMETRY_FILENAME_STEM, tmp_uuid)))
}

fn build_shapefile_reader<'a>(
    shapefile_bytes: &'a ShapefileBytes,
) -> Result<Reader<Cursor<&'a [u8]>, Cursor<&'a [u8]>>, AppError> {
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
