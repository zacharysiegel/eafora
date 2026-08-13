//! Held in memory. No temp files between download and FlatGeobuf emission.

use std::io::{Cursor, Read};

use crate::error::AppError;
use const_format::concatcp;
use zip::read::ZipFile;

const SHAPEFILE_BASENAME: &str = "ne_50m_admin_0_countries";
const NATURAL_EARTH_URL: &str = concatcp!(
    "https://naciscdn.org/naturalearth/50m/cultural/",
    SHAPEFILE_BASENAME,
    ".zip"
);

/// Natural Earth's `ADM0_A3` diverges from ISO 3166-1 alpha-3 for a few disputed or newly independent
/// states, and Natural Earth ships two unrecognized territories as their own features that we render as
/// part of their internationally recognized sovereign. Each pair maps the Natural Earth code to the
/// canonical ISO3 the seed keys on; a code not listed here already equals its ISO3.
const ADM0_A3_TO_CANONICAL_ISO3: &[(&str, &str)] = &[
    ("SDS", "SSD"), // South Sudan
    ("SAH", "ESH"), // Western Sahara
    ("PSX", "PSE"), // Palestine
    ("ALD", "ALA"), // Åland Islands
    ("KOS", "XKX"), // Kosovo has no ISO 3166-1 code; XKX is the code the World Bank uses, matched so its data joins
    ("SOL", "SOM"), // Somaliland, folded into Somalia
    ("CYN", "CYP"), // Northern Cyprus, folded into Cyprus
];

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

pub fn extract_shapefile_from_zip(zip_bytes: &[u8]) -> Result<ShapefileBytes, AppError> {
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

/// The canonical ISO3 the seed keys on for a Natural Earth `ADM0_A3` code, translating the codes that
/// diverge from ISO 3166-1 alpha-3 (see `ADM0_A3_TO_CANONICAL_ISO3`) and returning the code unchanged
/// otherwise.
pub fn canonical_iso3(adm0_a3: &str) -> &str {
    ADM0_A3_TO_CANONICAL_ISO3.iter()
        .find(|(natural_earth_code, _)| *natural_earth_code == adm0_a3)
        .map(|(_, canonical_iso3)| *canonical_iso3)
        .unwrap_or(adm0_a3)
}
