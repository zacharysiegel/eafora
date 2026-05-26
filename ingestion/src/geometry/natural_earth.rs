//! Pinned-release fetch + in-memory unzip of the Natural Earth 50m admin-0
//! countries dataset. The artifact builder consumes the four shapefile
//! components (`.shp`, `.shx`, `.dbf`, `.prj`) without ever touching disk
//! between download and FlatGeobuf emission.

use std::io::{Cursor, Read};

use crate::error::AppError;

pub const NATURAL_EARTH_URL: &str =
    "https://naciscdn.org/naturalearth/50m/cultural/ne_50m_admin_0_countries.zip";

const SHAPEFILE_BASENAME: &str = "ne_50m_admin_0_countries";

#[derive(Debug, Clone)]
pub struct ShapefileBytes {
    pub shp: Vec<u8>,
    pub shx: Vec<u8>,
    pub dbf: Vec<u8>,
    pub prj: Vec<u8>,
}

pub async fn download_pinned_release(client: &reqwest::Client) -> Result<Vec<u8>, AppError> {
    let response: reqwest::Response = client
        .get(NATURAL_EARTH_URL)
        .send()
        .await?
        .error_for_status()?;
    let bytes: Vec<u8> = response.bytes().await?.to_vec();
    Ok(bytes)
}

pub fn extract_shapefile_from_zip(zip_bytes: &[u8]) -> Result<ShapefileBytes, AppError> {
    let cursor: Cursor<&[u8]> = Cursor::new(zip_bytes);
    let mut archive: zip::ZipArchive<Cursor<&[u8]>> = zip::ZipArchive::new(cursor)
        .map_err(|err| AppError::from(format!("extract_shapefile_from_zip: open: {}", err)))?;

    let shp: Vec<u8> = read_named_entry(&mut archive, "shp")?;
    let shx: Vec<u8> = read_named_entry(&mut archive, "shx")?;
    let dbf: Vec<u8> = read_named_entry(&mut archive, "dbf")?;
    let prj: Vec<u8> = read_named_entry(&mut archive, "prj")?;

    Ok(ShapefileBytes { shp, shx, dbf, prj })
}

fn read_named_entry(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    extension: &str,
) -> Result<Vec<u8>, AppError> {
    let entry_name: String = format!("{}.{}", SHAPEFILE_BASENAME, extension);
    let mut entry: zip::read::ZipFile<'_, Cursor<&[u8]>> = archive.by_name(&entry_name).map_err(|err| {
        AppError::from(format!("extract_shapefile_from_zip: {}: {}", entry_name, err))
    })?;

    let mut buffer: Vec<u8> = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut buffer)
        .map_err(|err| AppError::from(format!("extract_shapefile_from_zip: read {}: {}", entry_name, err)))?;
    Ok(buffer)
}
