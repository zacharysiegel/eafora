//! Load a statistic shard's values into memory. A shard is one (statistic, license class)'s time
//! series across regions and periods; the renderer loads it once per bundle swap and then looks up
//! values per (country, period) from the in-memory map — no per-frame SQLite.
//!
//! Native reads the shard's in-memory bytes through rusqlite's `deserialize` (a safe `Connection`
//! over a SQLite-owned copy of the buffer); the wasm32 loader (raw `sqlite3_deserialize` against
//! `sqlite-wasm-rs`) lands in a later increment behind the same `load_shard` signature.

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::error::AppError;

/// The values of one statistic shard, keyed by country ISO 3166 alpha-3 and period, with the
/// value range precomputed for the choropleth color scale.
#[derive(Debug, Clone)]
pub struct ShardValues {
    by_region: HashMap<String, HashMap<NaiveDate, f64>>,
    min: f64,
    max: f64,
}

impl ShardValues {
    pub fn value(&self, region_iso3: &str, period: NaiveDate) -> Option<f64> {
        self.by_region.get(region_iso3)?.get(&period).copied()
    }

    pub fn range(&self) -> Option<(f64, f64)> {
        if self.by_region.is_empty() {
            return None;
        }

        Some((self.min, self.max))
    }
}

/// Read every `(region_iso3, period_start, value)` row of a statistic shard into a [`ShardValues`].
/// The shard's SQLite header is validated before any query per [`crate::sqlite::schema::validate_shard_header`].
#[cfg(not(target_arch = "wasm32"))] // not for wasm32: rusqlite doesn't compile there; the wasm loader lands separately
pub fn load_shard(bytes: &[u8]) -> Result<ShardValues, AppError> {
    use crate::sqlite::schema;

    let connection: rusqlite::Connection = deserialize_read_only(bytes)?;
    schema::validate_shard_header(&connection)?;

    let query: String = format!(
        "select {}, {}, {} from {}",
        schema::COL_REGION_ISO3,
        schema::COL_PERIOD_START,
        schema::COL_VALUE,
        schema::TABLE_STATISTIC_VALUE,
    );

    let mut statement: rusqlite::Statement<'_> = connection.prepare(&query)?;
    let row_iter = statement.query_map([], |row| {
        let region_iso3: String = row.get(0)?;
        let period_start: String = row.get(1)?;
        let value: f64 = row.get(2)?;

        Ok((region_iso3, period_start, value))
    })?;

    let mut by_region: HashMap<String, HashMap<NaiveDate, f64>> = HashMap::new();
    let mut min: f64 = f64::INFINITY;
    let mut max: f64 = f64::NEG_INFINITY;

    for row in row_iter {
        let (region_iso3, period_start, value): (String, String, f64) = row?;
        let period: NaiveDate = NaiveDate::parse_from_str(&period_start, schema::PERIOD_DATE_FORMAT)
            .map_err(|err| AppError::from(format!("shard_db: unparseable period_start {:?}: {}", period_start, err)))?;

        by_region.entry(region_iso3).or_default().insert(period, value);
        min = min.min(value);
        max = max.max(value);
    }

    Ok(ShardValues { by_region, min, max })
}

/// Open a read-only `Connection` over the shard's in-memory bytes. rusqlite's `deserialize` takes a
/// SQLite-owned (`sqlite3_malloc`'d) buffer that it frees on close, so the bytes are copied into one;
/// `read_only` = true since shards are immutable.
#[cfg(not(target_arch = "wasm32"))] // not for wasm32: uses rusqlite + libsqlite3-sys
fn deserialize_read_only(bytes: &[u8]) -> Result<rusqlite::Connection, AppError> {
    use std::ptr::NonNull;

    use rusqlite::serialize::OwnedData;
    use rusqlite::{Connection, DatabaseName};

    let mut connection: Connection = Connection::open_in_memory()?;

    let byte_count: usize = bytes.len();
    let owned_data: OwnedData = unsafe {
        let raw: *mut u8 = rusqlite::ffi::sqlite3_malloc(byte_count as std::os::raw::c_int) as *mut u8;
        let raw: NonNull<u8> = NonNull::new(raw).ok_or_else(|| AppError::from("shard_db: sqlite3_malloc returned null"))?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), raw.as_ptr(), byte_count);

        OwnedData::from_raw_nonnull(raw, byte_count)
    };

    connection.deserialize(DatabaseName::Main, owned_data, true)?;

    Ok(connection)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    use rusqlite::{Connection, DatabaseName};

    use crate::sqlite::schema;

    /// Build a real shard in memory via the shared DDL, then serialize it to bytes — so the loader
    /// round-trip is tested against the actual schema without committing an opaque binary.
    fn sample_shard_bytes() -> Vec<u8> {
        let connection: Connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(schema::shard_schema_ddl()).unwrap();
        connection.pragma_update(None, "application_id", schema::APPLICATION_ID).unwrap();
        connection.pragma_update(None, "user_version", schema::SCHEMA_VERSION).unwrap();

        let insert: String = format!(
            "insert into {} ({}, {}, {}, {}, {}, {}, {}, {}) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            schema::TABLE_STATISTIC_VALUE,
            schema::COL_REGION_ISO3,
            schema::COL_REGION_ID,
            schema::COL_PERIOD_START,
            schema::COL_PERIOD_END,
            schema::COL_VALUE,
            schema::COL_DATA_STATUS,
            schema::COL_DATA_SOURCE_CODE,
            schema::COL_DATA_SOURCE_REVISION,
        );
        let region_id: Vec<u8> = vec![0u8; 16];
        let rows: [(&str, &str, &str, f64); 3] = [
            ("USA", "2020-01-01", "2020-12-31", 1.6),
            ("USA", "2021-01-01", "2021-12-31", 1.7),
            ("DEU", "2020-01-01", "2020-12-31", 1.5),
        ];
        for (region_iso3, period_start, period_end, value) in rows {
            connection
                .execute(
                    &insert,
                    (region_iso3, region_id.clone(), period_start, period_end, value, "final", "wb_wdi", "2024-12-12"),
                )
                .unwrap();
        }

        let data: rusqlite::serialize::Data<'_> = connection.serialize(DatabaseName::Main).unwrap();

        data.to_vec()
    }

    #[test]
    fn load_shard_reads_values_and_range() {
        let shard: ShardValues = load_shard(&sample_shard_bytes()).unwrap();

        assert_eq!(shard.value("USA", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.6));
        assert_eq!(shard.value("USA", NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()), Some(1.7));
        assert_eq!(shard.value("DEU", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.5));
        assert_eq!(shard.range(), Some((1.5, 1.7)));
    }

    #[test]
    fn load_shard_returns_none_for_absent_region_and_period() {
        let shard: ShardValues = load_shard(&sample_shard_bytes()).unwrap();

        assert_eq!(shard.value("XKX", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), None);
        assert_eq!(shard.value("USA", NaiveDate::from_ymd_opt(1999, 1, 1).unwrap()), None);
    }

    #[test]
    fn load_shard_rejects_non_eafora_bytes() {
        let result: Result<ShardValues, AppError> = load_shard(b"not a sqlite database");

        assert!(result.is_err());
    }
}
