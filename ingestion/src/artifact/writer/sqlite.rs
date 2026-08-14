//! Schema mirrors the Postgres `statistic_value` shape but is denormalized
//! for client-side reads: `region_code` is duplicated for human-readable
//! queries, `region_id` is kept as a BLOB for the rare cross-shard joins,
//! periods are stored as ISO-8601 strings so client SQL doesn't need
//! date-function support.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use uuid::Uuid;

use const_format::formatcp;

use shared::artifact::bundle::StatisticShardKey;
use shared::canonical::canonical_model::{LicenseShardClass, StatisticKind};
use shared::filesystem::FileReference;
use shared::sqlite::schema;

use crate::artifact::artifact_model::{ResolvedValue, StatisticShard};
use crate::error::AppError;

pub fn write_sqlite_shards(
    values: &[ResolvedValue],
    data_dir: &Path,
) -> Result<Vec<StatisticShard<FileReference>>, AppError> {
    fs::create_dir_all(data_dir)?;

    let groups: BTreeMap<StatisticShardKey, Vec<&ResolvedValue>> = group_values(values);
    let shards: Vec<StatisticShard<FileReference>> = shard_values(data_dir, groups)?;
    Ok(shards)
}

fn group_values(resolved: &[ResolvedValue]) -> BTreeMap<StatisticShardKey, Vec<&ResolvedValue>> {
    let mut grouped: BTreeMap<StatisticShardKey, Vec<&ResolvedValue>> = BTreeMap::new();
    for resolved_value in resolved {
        grouped.entry(resolved_value.shard_key()).or_default().push(resolved_value);
    }
    grouped
}

fn shard_values(data_dir: &Path, grouped: BTreeMap<StatisticShardKey, Vec<&ResolvedValue>>) -> Result<Vec<StatisticShard<FileReference>>, AppError> {
    let mut shards: Vec<StatisticShard<FileReference>> = Vec::with_capacity(grouped.len());
    for (shard_key, values) in grouped {
        let file: FileReference = write_one_shard(data_dir, shard_key.statistic_kind, shard_key.license_shard_class, &values)?;
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
    connection.pragma_update(None, "application_id", schema::APPLICATION_ID)?;
    connection.pragma_update(None, "user_version", schema::SCHEMA_VERSION)?;

    connection.execute_batch(schema::shard_schema_ddl())?;
    insert_shard_key(&connection, statistic_kind, license_shard_class)?;
    insert_rows(&mut connection, values)?;

    let byte_count: u64 = fs::metadata(&path)?.len();

    Ok(FileReference { path, byte_count })
}

fn insert_shard_key(
    connection: &Connection,
    statistic_kind: StatisticKind,
    license_shard_class: LicenseShardClass,
) -> Result<(), AppError> {
    connection.execute(
        formatcp!(
            "insert into {} ({}, {}) values (?1, ?2)",
            schema::TABLE_SHARD_KEY, schema::COL_STATISTIC_KIND, schema::COL_LICENSE_SHARD_CLASS,
        ),
        (statistic_kind.code(), license_shard_class.as_str()),
    )?;
    Ok(())
}

fn insert_rows(connection: &mut Connection, values: &[&ResolvedValue]) -> Result<(), AppError> {
    let transaction: rusqlite::Transaction = connection.transaction()?;

    let mut statement: rusqlite::Statement = transaction.prepare(formatcp!(
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
    ))?;

    for resolved_value in values {
        statement.execute((
            &resolved_value.region_code,
            resolved_value.region_id.as_bytes().as_slice(),
            resolved_value.period.start.format(schema::PERIOD_DATE_FORMAT).to_string(),
            resolved_value.period.end.format(schema::PERIOD_DATE_FORMAT).to_string(),
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

    use shared::canonical::canonical_model::{DataSourceKind, DataStatus, NaiveDatePeriod};

    fn make_merged(
        statistic_kind: StatisticKind,
        license_shard_class: LicenseShardClass,
        region_code: &str,
        year: i32,
        value: f64,
    ) -> ResolvedValue {
        ResolvedValue {
            region_id: Uuid::from_u128(year as u128),
            region_code: region_code.to_string(),
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
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "usa", 2022, 1.66),
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "jpn", 2022, 1.30),
            make_merged(StatisticKind::Tfr, LicenseShardClass::NonCommercial, "usa", 2022, 1.66),
            make_merged(StatisticKind::TestAlpha, LicenseShardClass::Base, "usa", 2022, 1.85),
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
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "usa", 2022, 1.66),
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "jpn", 2022, 1.30),
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
                "select value from statistic_value where region_code = 'usa'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!((usa_value - 1.66).abs() < f64::EPSILON);

        let usa_period_start: String = connection
            .query_row(
                "select period_start from statistic_value where region_code = 'usa'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(usa_period_start, "2022-01-01");

        let region_id_bytes: Vec<u8> = connection
            .query_row(
                "select region_id from statistic_value where region_code = 'usa'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(region_id_bytes.len(), 16);
    }

    #[test]
    fn write_sqlite_shards_index_is_present() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<ResolvedValue> = vec![make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "usa", 2022, 1.66)];

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
            "usa",
            2022,
            1.66,
        )];

        let shards: Vec<StatisticShard<FileReference>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();
        let connection: Connection = Connection::open(&shards[0].file.path).unwrap();

        let application_id: i32 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .unwrap();
        assert_eq!(application_id, schema::APPLICATION_ID);

        let user_version: i32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, schema::SCHEMA_VERSION);

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
            "usa",
            2022,
            1.66,
        )];

        let shards: Vec<StatisticShard<FileReference>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        let filename: &str = shards[0].file.path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("tfr-share_alike.tmp-"));
    }
}
