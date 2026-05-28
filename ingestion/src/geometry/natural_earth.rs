//! Held in memory — no temp files between download and FlatGeobuf emission.

use std::io::{Cursor, Read};

use crate::error::AppError;
use const_format::concatcp;
use zip::read::ZipFile;
use crate::geometry::natural_earth;

const SHAPEFILE_BASENAME: &str = "ne_50m_admin_0_countries";
const NATURAL_EARTH_URL: &str = concatcp!(
    "https://naciscdn.org/naturalearth/50m/cultural/",
    SHAPEFILE_BASENAME,
    ".zip"
);

#[derive(Debug, Clone)]
pub struct ShapefileBytes {
    pub shp: Vec<u8>,
    pub shx: Vec<u8>,
    pub dbf: Vec<u8>,
    pub prj: Vec<u8>,
}

pub async fn download_pinned_release(client: &reqwest::Client) -> Result<ShapefileBytes, AppError> {
    let response: reqwest::Response = client.get(NATURAL_EARTH_URL).send().await?.error_for_status()?;
    let zip_bytes: Vec<u8> = response.bytes().await?.to_vec();
    let shapefile_bytes: ShapefileBytes = extract_shapefile_from_zip(&zip_bytes)?;
    Ok(shapefile_bytes)
}

fn extract_shapefile_from_zip(zip_bytes: &[u8]) -> Result<ShapefileBytes, AppError> {
    let cursor: Cursor<&[u8]> = Cursor::new(zip_bytes);
    let mut archive: zip::ZipArchive<Cursor<&[u8]>> = zip::ZipArchive::new(cursor)?;

    Ok(ShapefileBytes {
        shp: read_named_entry(&mut archive, "shp")?,
        shx: read_named_entry(&mut archive, "shx")?,
        dbf: read_named_entry(&mut archive, "dbf")?,
        prj: read_named_entry(&mut archive, "prj")?,
    })
}

fn read_named_entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, extension: &str) -> Result<Vec<u8>, AppError> {
    let entry_name: String = format!("{}.{}", SHAPEFILE_BASENAME, extension);
    let mut entry: ZipFile<'_, Cursor<&[u8]>> = archive.by_name(&entry_name)?;

    let mut buffer: Vec<u8> = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buffer)?;
    Ok(buffer)
}
