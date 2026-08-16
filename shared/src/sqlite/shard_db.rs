//! Load a statistic shard's bytes into an in-memory `(region, period_start) -> cell` map with its
//! value and period ranges precomputed.
//!
//! Both paths load the shard entirely into memory: the non-wasm32 path through rusqlite's
//! `deserialize`, wasm32 through the read-only VFS facade in `crate::sqlite::ro_memory_vfs`. Each is
//! a target-gated submodule scoping its own bindings; both re-export the one `read_shard` signature.

use std::collections::HashMap;

use chrono::NaiveDate;

#[derive(Debug, Clone)]
pub struct CellValue {
    pub value: f64,
    pub source_code: String,
    pub source_revision: String,
}

/// The values of one statistic shard, keyed by `region.code` and period start, with the value and
/// period ranges precomputed.
#[derive(Debug, Clone)]
pub struct ShardValues {
    by_region: HashMap<String, HashMap<NaiveDate, CellValue>>,
    min: f64,
    max: f64,
    earliest_period_start: NaiveDate,
    latest_period_start: NaiveDate,
}

impl ShardValues {
    pub fn value(&self, region_code: &str, period_start: NaiveDate) -> Option<f64> {
        self.cell(region_code, period_start).map(|cell| cell.value)
    }

    pub fn cell(&self, region_code: &str, period_start: NaiveDate) -> Option<&CellValue> {
        self.by_region.get(region_code)?.get(&period_start)
    }

    pub fn value_range(&self) -> Option<(f64, f64)> {
        if self.by_region.is_empty() {
            return None;
        }

        Some((self.min, self.max))
    }

    pub fn period_range(&self) -> Option<(NaiveDate, NaiveDate)> {
        if self.by_region.is_empty() {
            return None;
        }

        Some((self.earliest_period_start, self.latest_period_start))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::HashMap;
    use std::ptr::NonNull;

    use chrono::NaiveDate;

    use rusqlite::serialize::OwnedData;
    use rusqlite::{Connection, DatabaseName};

    use crate::error::AppError;
    use crate::sqlite::schema;

    use super::{CellValue, ShardValues};

    /// Read every `(region_code, period_start, value)` row of a statistic shard into a [`ShardValues`].
    /// The shard's SQLite header is validated before any query per [`crate::sqlite::schema::validate_shard_header`].
    pub fn read_shard(bytes: &[u8]) -> Result<ShardValues, AppError> {
        let connection: Connection = deserialize_read_only(bytes)?;
        schema::validate_shard_header(&connection)?;

        let query: String = format!(
            "select {}, {}, {}, {}, {} from {}",
            schema::COL_REGION_CODE,
            schema::COL_PERIOD_START,
            schema::COL_VALUE,
            schema::COL_DATA_SOURCE_CODE,
            schema::COL_DATA_SOURCE_REVISION,
            schema::TABLE_STATISTIC_VALUE,
        );

        let mut statement: rusqlite::Statement<'_> = connection.prepare(&query)?;
        let row_iter = statement.query_map([], |row| {
            let region_code: String = row.get(0)?;
            let period_start: String = row.get(1)?;
            let value: f64 = row.get(2)?;
            let source_code: String = row.get(3)?;
            let source_revision: String = row.get(4)?;

            Ok((region_code, period_start, value, source_code, source_revision))
        })?;

        let mut by_region: HashMap<String, HashMap<NaiveDate, CellValue>> = HashMap::new();
        let mut min: f64 = f64::INFINITY;
        let mut max: f64 = f64::NEG_INFINITY;
        let mut earliest_period_start: NaiveDate = NaiveDate::MAX;
        let mut latest_period_start: NaiveDate = NaiveDate::MIN;

        for row in row_iter {
            let (region_code, period_start, value, source_code, source_revision): (String, String, f64, String, String) = row?;
            let period_start: NaiveDate = NaiveDate::parse_from_str(&period_start, schema::PERIOD_DATE_FORMAT)
                .map_err(|err| AppError::from(format!("shard_db: unparseable period_start {:?}: {}", period_start, err)))?;

            let cell: CellValue = CellValue { value, source_code, source_revision };
            by_region.entry(region_code).or_default().insert(period_start, cell);
            min = min.min(value);
            max = max.max(value);
            earliest_period_start = earliest_period_start.min(period_start);
            latest_period_start = latest_period_start.max(period_start);
        }

        Ok(ShardValues { by_region, min, max, earliest_period_start, latest_period_start })
    }

    /// Open a read-only `Connection` over the shard's in-memory bytes. rusqlite's `deserialize` takes a
    /// SQLite-owned (`sqlite3_malloc`'d) buffer that it frees on close, so the bytes are copied into one;
    /// `read_only` = true since shards are immutable.
    fn deserialize_read_only(bytes: &[u8]) -> Result<Connection, AppError> {
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
}

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::collections::HashMap;
    use std::ffi::CString;

    use chrono::NaiveDate;

    use sqlite_wasm_rs::{sqlite3, sqlite3_stmt};

    use crate::error::AppError;
    use crate::sqlite::{ffi_conversions, ro_memory_vfs, schema};

    use super::{CellValue, ShardValues};

    /// Open the shard's bytes through the read-only VFS (native has no such module) and step the one
    /// load-once query via the raw `sqlite3_*` FFI, since `sqlite-wasm-rs` exposes no safe query
    /// wrapper. Same result shape as the native path.
    pub fn read_shard(bytes: &[u8]) -> Result<ShardValues, AppError> {
        let filename: String = ro_memory_vfs::register_shard(bytes);
        let result: Result<ShardValues, AppError> = open_and_read_shard(&filename);
        ro_memory_vfs::unregister_shard(&filename);

        result
    }

    fn open_and_read_shard(vfs_filename: &str) -> Result<ShardValues, AppError> {
        let filename: CString = CString::new(vfs_filename).expect("no null byte");
        let mut db: *mut sqlite3 = std::ptr::null_mut();

        let open_res: std::os::raw::c_int =
            unsafe { sqlite_wasm_rs::sqlite3_open_v2(filename.as_ptr(), &mut db, sqlite_wasm_rs::SQLITE_OPEN_READONLY, ro_memory_vfs::VFS_NAME.as_ptr()) };
        if open_res != sqlite_wasm_rs::SQLITE_OK {
            let message: String = ffi_conversions::error_message(db);
            unsafe { sqlite_wasm_rs::sqlite3_close(db) };
            return Err(AppError::from(format!("shard_db: open failed: {message}")));
        }

        let result: Result<ShardValues, AppError> = read_all_rows(db);

        unsafe { sqlite_wasm_rs::sqlite3_close(db) };

        result
    }

    /// Owns a prepared statement and finalizes it on drop, so every early return releases it.
    struct Statement {
        handle: *mut sqlite3_stmt,
    }

    impl Drop for Statement {
        fn drop(&mut self) {
            unsafe { sqlite_wasm_rs::sqlite3_finalize(self.handle) };
        }
    }

    fn read_all_rows(db: *mut sqlite3) -> Result<ShardValues, AppError> {
        let query: CString = CString::new(format!(
            "select {}, {}, {}, {}, {} from {}",
            schema::COL_REGION_CODE,
            schema::COL_PERIOD_START,
            schema::COL_VALUE,
            schema::COL_DATA_SOURCE_CODE,
            schema::COL_DATA_SOURCE_REVISION,
            schema::TABLE_STATISTIC_VALUE,
        ))
        .unwrap();

        let mut raw_statement: *mut sqlite3_stmt = std::ptr::null_mut();
        let prepare_res: std::os::raw::c_int =
            unsafe { sqlite_wasm_rs::sqlite3_prepare_v2(db, query.as_ptr(), -1, &mut raw_statement, std::ptr::null_mut()) };
        if prepare_res != sqlite_wasm_rs::SQLITE_OK {
            let message: String = ffi_conversions::error_message(db);
            return Err(AppError::from(format!("shard_db: prepare failed: {message}")));
        }

        let statement: Statement = Statement { handle: raw_statement };

        let mut by_region: HashMap<String, HashMap<NaiveDate, CellValue>> = HashMap::new();
        let mut min: f64 = f64::INFINITY;
        let mut max: f64 = f64::NEG_INFINITY;
        let mut earliest_period_start: NaiveDate = NaiveDate::MAX;
        let mut latest_period_start: NaiveDate = NaiveDate::MIN;

        loop {
            let step_res: std::os::raw::c_int = unsafe { sqlite_wasm_rs::sqlite3_step(statement.handle) };
            if step_res == sqlite_wasm_rs::SQLITE_ROW {
                let region_code: String = ffi_conversions::column_text(statement.handle, 0)?;
                let period_start: String = ffi_conversions::column_text(statement.handle, 1)?;
                let value: f64 = unsafe { sqlite_wasm_rs::sqlite3_column_double(statement.handle, 2) };
                let source_code: String = ffi_conversions::column_text(statement.handle, 3)?;
                let source_revision: String = ffi_conversions::column_text(statement.handle, 4)?;
                let period_start: NaiveDate = NaiveDate::parse_from_str(&period_start, schema::PERIOD_DATE_FORMAT)
                    .map_err(|err| AppError::from(format!("shard_db: unparseable period_start {:?}: {}", period_start, err)))?;

                let cell: CellValue = CellValue { value, source_code, source_revision };
                by_region.entry(region_code).or_default().insert(period_start, cell);
                min = min.min(value);
                max = max.max(value);
                earliest_period_start = earliest_period_start.min(period_start);
                latest_period_start = latest_period_start.max(period_start);
            } else if step_res == sqlite_wasm_rs::SQLITE_DONE {
                break;
            } else {
                let message: String = ffi_conversions::error_message(db);
                return Err(AppError::from(format!("shard_db: step failed: {message}")));
            }
        }

        Ok(ShardValues { by_region, min, max, earliest_period_start, latest_period_start })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    use rusqlite::{Connection, DatabaseName};

    use crate::error::AppError;
    use crate::sqlite::schema;

    /// Build a real shard in memory via the shared DDL, then serialize it to bytes. The loader
    /// round-trip is then tested against the actual schema without committing an opaque binary.
    fn sample_shard_bytes() -> Vec<u8> {
        let connection: Connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(schema::shard_schema_ddl()).unwrap();
        connection.pragma_update(None, "application_id", schema::APPLICATION_ID).unwrap();
        connection.pragma_update(None, "user_version", schema::SCHEMA_VERSION).unwrap();

        let insert: String = format!(
            "insert into {} ({}, {}, {}, {}, {}, {}, {}, {}) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            schema::TABLE_STATISTIC_VALUE,
            schema::COL_REGION_CODE,
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
            ("usa", "2020-01-01", "2020-12-31", 1.6),
            ("usa", "2021-01-01", "2021-12-31", 1.7),
            ("deu", "2020-01-01", "2020-12-31", 1.5),
        ];
        for (region_code, period_start, period_end, value) in rows {
            connection
                .execute(
                    &insert,
                    (region_code, region_id.clone(), period_start, period_end, value, "final", "wb_wdi", "2024-12-12"),
                )
                .unwrap();
        }

        let data: rusqlite::serialize::Data<'_> = connection.serialize(DatabaseName::Main).unwrap();

        data.to_vec()
    }

    #[test]
    fn read_shard_reads_values_and_range() {
        let shard: ShardValues = read_shard(&sample_shard_bytes()).unwrap();

        assert_eq!(shard.value("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.6));
        assert_eq!(shard.value("usa", NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()), Some(1.7));
        assert_eq!(shard.value("deu", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.5));
        assert_eq!(shard.value_range(), Some((1.5, 1.7)));
        assert_eq!(
            shard.period_range(),
            Some((NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(), NaiveDate::from_ymd_opt(2021, 1, 1).unwrap())),
        );
    }

    #[test]
    fn read_shard_reads_cell_source() {
        let shard: ShardValues = read_shard(&sample_shard_bytes()).unwrap();

        let cell: &CellValue = shard.cell("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()).unwrap();
        assert_eq!(cell.value, 1.6);
        assert_eq!(cell.source_code, "wb_wdi");
        assert_eq!(cell.source_revision, "2024-12-12");
    }

    #[test]
    fn read_shard_returns_none_for_absent_region_and_period() {
        let shard: ShardValues = read_shard(&sample_shard_bytes()).unwrap();

        assert_eq!(shard.value("xkx", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), None);
        assert_eq!(shard.value("usa", NaiveDate::from_ymd_opt(1999, 1, 1).unwrap()), None);
    }

    #[test]
    fn read_shard_rejects_non_eafora_bytes() {
        let result: Result<ShardValues, AppError> = read_shard(b"not a sqlite database");

        assert!(result.is_err());
    }

    #[test]
    #[ignore = "run manually to regenerate tests/samples/tfr-sample.sqlite"]
    fn dump_sample_shard() {
        std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/tfr-sample.sqlite"),
            sample_shard_bytes(),
        )
        .unwrap();
    }
}

// The wasm loader can't build a fixture (no rusqlite there), so it reads the committed sample that
// the native `dump_sample_shard` produced. This is the one runtime wasm test: the VFS + raw-FFI
// query is the genuinely target-divergent surface (native goes through rusqlite instead).
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;

    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn read_shard_reads_committed_sample_through_the_vfs() {
        let bytes: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/tfr-sample.sqlite"));

        let shard: ShardValues = read_shard(bytes).unwrap();

        assert_eq!(shard.value("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.6));
        assert_eq!(shard.value("deu", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.5));
        assert_eq!(shard.value_range(), Some((1.5, 1.7)));
        assert_eq!(shard.value("xkx", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), None);
        assert_eq!(shard.cell("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()).unwrap().source_code, "wb_wdi");
    }
}
