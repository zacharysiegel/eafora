use const_format::formatcp;

// not for wasm32: the only consumer, validate_shard_header, is itself native-only
#[cfg(not(target_arch = "wasm32"))]
use crate::error::AppError;

/// ASCII "EAFO"; written to SQLite's `application_id` PRAGMA (offset 60) so hex
/// viewers and `file(1)` can identify Eafora shards by magic number alone.
pub const APPLICATION_ID: i32 = 0x4541464F;

/// Written to SQLite's `user_version` PRAGMA (offset 68). Bump when the shard
/// schema changes in a way consumers need to detect; same forward-compat
/// motivation as the manifest's `manifest_schema_version`.
pub const SCHEMA_VERSION: i32 = 1;

pub const TABLE_STATISTIC_VALUE: &str = "statistic_value";
pub const TABLE_SHARD_KEY: &str = "shard_key";

pub const INDEX_STATISTIC_VALUE_BY_REGION: &str = "statistic_value_by_region";

pub const COL_REGION_ISO3: &str = "region_iso3";
pub const COL_REGION_ID: &str = "region_id";
pub const COL_PERIOD_START: &str = "period_start";
pub const COL_PERIOD_END: &str = "period_end";
pub const COL_VALUE: &str = "value";
pub const COL_DATA_STATUS: &str = "data_status";
pub const COL_DATA_SOURCE_CODE: &str = "data_source_code";
pub const COL_DATA_SOURCE_REVISION: &str = "data_source_revision";

pub const COL_STATISTIC_KIND: &str = "statistic_kind";
pub const COL_LICENSE_SHARD_CLASS: &str = "license_shard_class";

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

create table {TABLE_STATISTIC_VALUE} (
    {COL_REGION_ISO3} text not null,
    {COL_REGION_ID} blob not null,
    {COL_PERIOD_START} text not null,
    {COL_PERIOD_END} text not null,
    {COL_VALUE} real not null,
    {COL_DATA_STATUS} text not null,
    {COL_DATA_SOURCE_CODE} text not null,
    {COL_DATA_SOURCE_REVISION} text not null,
    primary key ({COL_REGION_ISO3}, {COL_PERIOD_START}, {COL_PERIOD_END})
);
create index {INDEX_STATISTIC_VALUE_BY_REGION} on {TABLE_STATISTIC_VALUE} ({COL_REGION_ID});
"
    )
}

/// Consumer-side gate: confirm a connection's SQLite header marks it as an Eafora
/// shard with a schema version we understand, before issuing any query.
// not for wasm32: takes a rusqlite::Connection, and rusqlite doesn't compile to wasm32
#[cfg(not(target_arch = "wasm32"))]
pub fn validate_shard_header(connection: &rusqlite::Connection) -> Result<(), AppError> {
    let application_id: i32 = connection.pragma_query_value(None, "application_id", |row| row.get::<_, i32>(0))?;
    if application_id != APPLICATION_ID {
        return Err(AppError::from(format!(
            "sqlite shard: application_id mismatch (got {:#x}, expected {:#x})",
            application_id, APPLICATION_ID,
        )));
    }

    let user_version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))?;
    if user_version != SCHEMA_VERSION {
        return Err(AppError::from(format!(
            "sqlite shard: unknown schema_version {} (expected {})",
            user_version, SCHEMA_VERSION,
        )));
    }

    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    use crate::error::AppError;

    fn shard_with_header(application_id: i32, user_version: i32) -> rusqlite::Connection {
        let connection: rusqlite::Connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "application_id", application_id).unwrap();
        connection.pragma_update(None, "user_version", user_version).unwrap();
        connection
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

        let region_iso3_count: i64 = connection
            .query_row(
                formatcp!("select count(*) from pragma_table_info('{TABLE_STATISTIC_VALUE}') where name = '{COL_REGION_ISO3}'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(region_iso3_count, 1);
    }

    #[test]
    fn validate_shard_header_accepts_correctly_initialized_connection() {
        let connection: rusqlite::Connection = shard_with_header(APPLICATION_ID, SCHEMA_VERSION);

        validate_shard_header(&connection).unwrap();
    }

    #[test]
    fn validate_shard_header_rejects_wrong_application_id() {
        let connection: rusqlite::Connection = shard_with_header(0xDEADBEEFu32 as i32, SCHEMA_VERSION);

        let error: AppError = validate_shard_header(&connection).unwrap_err();

        assert!(error.to_string().contains("sqlite shard: application_id mismatch"));
    }

    #[test]
    fn validate_shard_header_rejects_unknown_schema_version() {
        let connection: rusqlite::Connection = shard_with_header(APPLICATION_ID, 99);

        let error: AppError = validate_shard_header(&connection).unwrap_err();

        assert!(error.to_string().contains("sqlite shard: unknown schema_version"));
    }
}
