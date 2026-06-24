//! Schema mirrors the Postgres `statistic_value` shape but is denormalized
//! for client-side reads: `region_iso3` is duplicated for human-readable
//! queries, `region_id` is kept as a BLOB for the rare cross-shard joins,
//! periods are stored as ISO-8601 strings so client SQL doesn't need
//! date-function support.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use uuid::Uuid;

use crate::artifact::artifact_model::{ResolvedValue, StatisticShard, StatisticShardKey};
use shared::filesystem::FileReference;
use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};
use crate::error::AppError;

/// Magic number written into SQLite's 32-bit `application_id` header field
/// at offset 60. Spells `"EAFO"` in ASCII so that hex viewers and tools like
/// `file(1)` (with the right magic-database entry) can identify these as
/// Eafora shards independent of filename or context.
const SQLITE_APPLICATION_ID: i32 = 0x4541464F;

/// Schema version stored in SQLite's `user_version` header field at offset
/// 68. Bump when the shard schema changes in a way clients need to detect.
const SQLITE_USER_VERSION: i32 = 0x1;

pub fn write_sqlite_shards(
    values: &[ResolvedValue],
    data_dir: &Path,
) -> Result<Vec<StatisticShard<FileReference>>, AppError> {
    fs::create_dir_all(&data_dir)?;

    let groups: BTreeMap<StatisticShardKey, Vec<&ResolvedValue>> = group_values(values);
    let shards: Vec<StatisticShard<FileReference>> = shard_values(&data_dir, groups)?;
    Ok(shards)
}

fn group_values(resolved: &[ResolvedValue]) -> BTreeMap<StatisticShardKey, Vec<&ResolvedValue>> {
    let mut grouped: BTreeMap<StatisticShardKey, Vec<&ResolvedValue>> = BTreeMap::new();
    for resolved_value in resolved {
        grouped.entry(StatisticShardKey::from_value(resolved_value)).or_default().push(resolved_value);
    }
    grouped
}

fn shard_values(data_dir: &Path, grouped: BTreeMap<StatisticShardKey, Vec<&ResolvedValue>>) -> Result<Vec<StatisticShard<FileReference>>, AppError> {
    let mut shards: Vec<StatisticShard<FileReference>> = Vec::with_capacity(grouped.len());
    for (shard_key, values) in grouped {
        let file: FileReference = write_one_shard(&data_dir, shard_key.statistic_kind, shard_key.license_shard_class, &values)?;
        shards.push(StatisticShard {
            key: shard_key,
            file,
        });
    }
    Ok(shards)
}

fn write_one_shard(
    data_dir: &Path,
    statistic_kind: StatisticKind,
    license_shard_class: LicenseShardClass,
    values: &[&ResolvedValue],
) -> Result<FileReference, AppError> {
    let tmp_uuid: Uuid = Uuid::now_v7();
    let filename: String = format!(
        "{}-{}.tmp-{}.sqlite",
        statistic_kind.code(),
        license_shard_class.as_str(),
        tmp_uuid,
    );
    let path: PathBuf = data_dir.join(&filename);

    let mut connection: Connection = Connection::open(&path)?;
    connection.pragma_update(None, "journal_mode", "MEMORY")?;
    connection.pragma_update(None, "application_id", SQLITE_APPLICATION_ID)?;
    connection.pragma_update(None, "user_version", SQLITE_USER_VERSION)?;

    create_schema(&connection)?;
    insert_shard_key(&connection, statistic_kind, license_shard_class)?;
    insert_rows(&mut connection, values)?;

    let byte_count: u64 = fs::metadata(&path)?.len();

    Ok(FileReference { path, byte_count })
}

fn create_schema(connection: &Connection) -> Result<(), AppError> {
    connection.execute_batch(
        r#"
        create table shard_key (
            statistic_kind      text not null,
            license_shard_class text not null
        );

        create table statistic_value (
            region_iso3          text not null,
            region_id            blob not null,
            period_start         text not null,
            period_end           text not null,
            value                real not null,
            data_status          text not null,
            data_source_code     text not null,
            data_source_revision text not null,
            primary key (region_iso3, period_start, period_end)
        );
        create index statistic_value_by_region on statistic_value (region_id);
        "#,
    )?;
    Ok(())
}

fn insert_shard_key(
    connection: &Connection,
    statistic_kind: StatisticKind,
    license_shard_class: LicenseShardClass,
) -> Result<(), AppError> {
    connection.execute(
        "insert into shard_key (statistic_kind, license_shard_class) values (?1, ?2)",
        (statistic_kind.code(), license_shard_class.as_str()),
    )?;
    Ok(())
}

fn insert_rows(connection: &mut Connection, values: &[&ResolvedValue]) -> Result<(), AppError> {
    let transaction: rusqlite::Transaction = connection.transaction()?;

    let mut statement: rusqlite::Statement = transaction.prepare(
        r#"
        insert into statistic_value
            (region_iso3, region_id, period_start, period_end, value,
             data_status, data_source_code, data_source_revision)
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
    )?;

    for resolved_value in values {
        statement.execute((
            &resolved_value.region_iso3,
            resolved_value.region_id.as_bytes().as_slice(),
            resolved_value.period.start.format("%Y-%m-%d").to_string(),
            resolved_value.period.end.format("%Y-%m-%d").to_string(),
            resolved_value.value,
            resolved_value.data_status.as_str(),
            resolved_value.data_source_kind.code(),
            &resolved_value.data_source_revision,
        ))?;
    }

    drop(statement);
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::adapter::adapter_model::NaiveDatePeriod;
    use crate::canonical::canonical_model::{DataSourceKind, DataStatus};

    fn make_merged(
        statistic_kind: StatisticKind,
        license_shard_class: LicenseShardClass,
        region_iso3: &str,
        year: i32,
        value: f64,
    ) -> ResolvedValue {
        ResolvedValue {
            region_id: Uuid::from_u128(year as u128),
            region_iso3: region_iso3.to_string(),
            statistic_kind,
            period: NaiveDatePeriod::from_year(year).unwrap(),
            value,
            data_status: DataStatus::Final,
            data_source_kind: DataSourceKind::WorldBankWDI,
            data_source_revision: "2024-Q4".to_string(),
            license_shard_class,
        }
    }

    #[test]
    fn write_sqlite_shards_creates_one_file_per_statistic_per_license_class() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<ResolvedValue> = vec![
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "USA", 2022, 1.66),
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "JPN", 2022, 1.30),
            make_merged(StatisticKind::Tfr, LicenseShardClass::NonCommercial, "USA", 2022, 1.66),
            make_merged(StatisticKind::TestAlpha, LicenseShardClass::Base, "USA", 2022, 1.85),
        ];

        let shards: Vec<StatisticShard<FileReference>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        assert_eq!(shards.len(), 3);
        for shard in &shards {
            assert!(shard.file.path.exists());
            assert!(shard.file.byte_count > 0);
            let filename: &str = shard.file.path.file_name().unwrap().to_str().unwrap();
            assert!(filename.contains(".tmp-"));
            assert!(filename.ends_with(".sqlite"));
        }
    }

    #[test]
    fn write_sqlite_shards_writes_rows_with_expected_schema() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<ResolvedValue> = vec![
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "USA", 2022, 1.66),
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "JPN", 2022, 1.30),
        ];

        let shards: Vec<StatisticShard<FileReference>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        assert_eq!(shards.len(), 1);
        let connection: Connection = Connection::open(&shards[0].file.path).unwrap();
        let row_count: i64 = connection
            .query_row("select count(*) from statistic_value", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, 2);

        let usa_value: f64 = connection
            .query_row(
                "select value from statistic_value where region_iso3 = 'USA'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!((usa_value - 1.66).abs() < f64::EPSILON);

        let usa_period_start: String = connection
            .query_row(
                "select period_start from statistic_value where region_iso3 = 'USA'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(usa_period_start, "2022-01-01");

        let region_id_bytes: Vec<u8> = connection
            .query_row(
                "select region_id from statistic_value where region_iso3 = 'USA'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(region_id_bytes.len(), 16);
    }

    #[test]
    fn write_sqlite_shards_index_is_present() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<ResolvedValue> = vec![make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "USA", 2022, 1.66)];

        let shards: Vec<StatisticShard<FileReference>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        let connection: Connection = Connection::open(&shards[0].file.path).unwrap();
        let index_count: i64 = connection
            .query_row(
                "select count(*) from sqlite_master where type='index' and name='statistic_value_by_region'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 1);
    }

    #[test]
    fn write_sqlite_shards_writes_header_and_shard_key() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<ResolvedValue> = vec![make_merged(
            StatisticKind::Tfr,
            LicenseShardClass::NonCommercial,
            "USA",
            2022,
            1.66,
        )];

        let shards: Vec<StatisticShard<FileReference>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();
        let connection: Connection = Connection::open(&shards[0].file.path).unwrap();

        let application_id: i32 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .unwrap();
        assert_eq!(application_id, SQLITE_APPLICATION_ID);

        let user_version: i32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, SQLITE_USER_VERSION);

        let (statistic_kind, license_shard_class): (String, String) = connection
            .query_row(
                "select statistic_kind, license_shard_class from shard_key",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(statistic_kind, "tfr");
        assert_eq!(license_shard_class, "noncommercial");
    }

    #[test]
    fn write_sqlite_shards_uses_correct_filename_format() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<ResolvedValue> = vec![make_merged(
            StatisticKind::Tfr,
            LicenseShardClass::ShareAlike,
            "USA",
            2022,
            1.66,
        )];

        let shards: Vec<StatisticShard<FileReference>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        let filename: &str = shards[0].file.path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("tfr-share_alike.tmp-"));
    }
}
