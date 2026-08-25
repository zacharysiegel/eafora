use const_format::formatcp;

use crate::artifact::schema_version;
use crate::error::AppError;

/// ASCII "EAFO"; written to SQLite's `application_id` PRAGMA so hex viewers and `file(1)` can identify
/// Eafora shards by magic number alone.
pub const APPLICATION_ID: i32 = 0x4541464F;

/// Written to SQLite's `user_version` PRAGMA. Bump when the shard
/// schema changes in a way consumers need to detect; same forward-compat
/// motivation as the manifest's `manifest_schema_version`.
pub const SCHEMA_VERSION: i32 = 2;

pub const TABLE_STATISTIC_VALUE: &str = "statistic_value";
pub const TABLE_SHARD_KEY: &str = "shard_key";
pub const TABLE_DATA_SOURCE: &str = "data_source";

pub const INDEX_STATISTIC_VALUE_BY_REGION: &str = "statistic_value_by_region";

pub const COL_REGION_CODE: &str = "region_code";
pub const COL_REGION_ID: &str = "region_id";
pub const COL_PERIOD_START: &str = "period_start";
pub const COL_PERIOD_END: &str = "period_end";
pub const COL_VALUE: &str = "value";
pub const COL_DATA_STATUS: &str = "data_status";
pub const COL_DATA_SOURCE_CODE: &str = "data_source_code";
pub const COL_DATA_SOURCE_REVISION: &str = "data_source_revision";

pub const COL_STATISTIC_KIND: &str = "statistic_kind";
pub const COL_LICENSE_SHARD_CLASS: &str = "license_shard_class";

pub const COL_PREFERENCE_RANK: &str = "preference_rank";

/// ISO 8601 date format for the `period_start` / `period_end` columns. Producer
/// formats periods with it; consumer SQL string-compares periods without needing
/// date-function support.
pub const PERIOD_DATE_FORMAT: &str = "%Y-%m-%d";

/// The shard schema DDL, composed at compile time from the table / column / index
/// name constants above so producer and consumer can never drift on a name.
pub fn shard_schema_ddl() -> &'static str {
    formatcp!(
        "create table {TABLE_SHARD_KEY} (
    {COL_STATISTIC_KIND} text not null,
    {COL_LICENSE_SHARD_CLASS} text not null
);

create table {TABLE_DATA_SOURCE} (
    {COL_DATA_SOURCE_CODE} text not null primary key,
    {COL_PREFERENCE_RANK} integer not null
);

create table {TABLE_STATISTIC_VALUE} (
    {COL_REGION_CODE} text not null,
    {COL_REGION_ID} blob not null,
    {COL_PERIOD_START} text not null,
    {COL_PERIOD_END} text not null,
    {COL_VALUE} real not null,
    {COL_DATA_STATUS} text not null,
    {COL_DATA_SOURCE_CODE} text not null references {TABLE_DATA_SOURCE} ({COL_DATA_SOURCE_CODE}),
    {COL_DATA_SOURCE_REVISION} text not null,
    primary key ({COL_REGION_CODE}, {COL_PERIOD_START}, {COL_PERIOD_END}, {COL_DATA_SOURCE_CODE})
);
create index {INDEX_STATISTIC_VALUE_BY_REGION} on {TABLE_STATISTIC_VALUE} ({COL_REGION_ID});
"
    )
}

/// SQLite writes both values at fixed offsets in a database file's header, so a shard is checked before any
/// handle is opened and both targets run the same check.
const SCHEMA_VERSION_OFFSET: usize = 60;
const APPLICATION_ID_OFFSET: usize = 68;

/// Consumer-side gate: confirm bytes are an Eafora shard at a schema version this build understands, before
/// issuing any query.
pub fn validate_shard_header(bytes: &[u8]) -> Result<(), AppError> {
    let application_id: i32 = read_header_i32(bytes, APPLICATION_ID_OFFSET)?;
    if application_id != APPLICATION_ID {
        return Err(AppError::from(format!(
            "sqlite shard: application_id mismatch (got {:#x}, expected {:#x})",
            application_id, APPLICATION_ID,
        )));
    }

    let schema_version: i32 = read_header_i32(bytes, SCHEMA_VERSION_OFFSET)?;
    if schema_version != SCHEMA_VERSION {
        return Err(AppError::from(format!(
            "sqlite shard: {}",
            schema_version::describe_mismatch("schema_version", schema_version, SCHEMA_VERSION),
        )));
    }

    Ok(())
}

fn read_header_i32(bytes: &[u8], offset: usize) -> Result<i32, AppError> {
    let field: Option<&[u8]> = bytes.get(offset..offset + 4);

    match field {
        Some(field) => Ok(i32::from_be_bytes([field[0], field[1], field[2], field[3]])),
        None => Err(AppError::from(format!(
            "sqlite shard: too short to hold a header; [bytes={}]",
            bytes.len(),
        ))),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    use crate::error::AppError;

    /// Serialized rather than hand-built, so the offsets under test are the ones SQLite itself writes.
    fn shard_bytes_with_header(application_id: i32, schema_version: i32) -> Vec<u8> {
        let connection: rusqlite::Connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(shard_schema_ddl()).unwrap();
        connection.pragma_update(None, "application_id", application_id).unwrap();
        connection.pragma_update(None, "user_version", schema_version).unwrap();

        let data: rusqlite::serialize::Data<'_> =
            connection.serialize(rusqlite::DatabaseName::Main).unwrap();

        data.to_vec()
    }

    #[test]
    fn shard_schema_ddl_creates_expected_tables_and_index() {
        let connection: rusqlite::Connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(shard_schema_ddl()).unwrap();

        let table_count: i64 = connection
            .query_row(
                formatcp!("select count(*) from sqlite_master where type = 'table' and name in ('{TABLE_SHARD_KEY}', '{TABLE_STATISTIC_VALUE}')"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 2);

        let index_count: i64 = connection
            .query_row(
                formatcp!("select count(*) from sqlite_master where type = 'index' and name = '{INDEX_STATISTIC_VALUE_BY_REGION}'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 1);

        let region_code_count: i64 = connection
            .query_row(
                formatcp!("select count(*) from pragma_table_info('{TABLE_STATISTIC_VALUE}') where name = '{COL_REGION_CODE}'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(region_code_count, 1);
    }

    #[test]
    fn validate_shard_header_accepts_a_shard_this_build_understands() {
        let bytes: Vec<u8> = shard_bytes_with_header(APPLICATION_ID, SCHEMA_VERSION);

        validate_shard_header(&bytes).unwrap();
    }

    #[test]
    fn validate_shard_header_rejects_wrong_application_id() {
        let bytes: Vec<u8> = shard_bytes_with_header(0xDEADBEEFu32 as i32, SCHEMA_VERSION);

        let error: AppError = validate_shard_header(&bytes).unwrap_err();

        assert!(error.to_string().contains("sqlite shard: application_id mismatch"));
    }

    /// The version a reader of the previous schema meets when handed a shard of this one.
    #[test]
    fn validate_shard_header_rejects_a_neighbouring_schema_version() {
        for schema_version in [SCHEMA_VERSION - 1, SCHEMA_VERSION + 1] {
            let bytes: Vec<u8> = shard_bytes_with_header(APPLICATION_ID, schema_version);

            let error: AppError = validate_shard_header(&bytes).unwrap_err();

            assert!(error.to_string().contains("sqlite shard: schema_version"));
        }
    }

    #[test]
    fn validate_shard_header_rejects_bytes_too_short_to_hold_a_header() {
        let error: AppError = validate_shard_header(&[0u8; 16]).unwrap_err();

        assert!(error.to_string().contains("too short"));
    }
}
