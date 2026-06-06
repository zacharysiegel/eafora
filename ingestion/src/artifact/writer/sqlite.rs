//! Schema mirrors the Postgres `statistic_value` shape but is denormalized
//! for client-side reads: `region_iso3` is duplicated for human-readable
//! queries, `region_id` is kept as a BLOB for the rare cross-shard joins,
//! periods are stored as ISO-8601 strings so client SQL doesn't need
//! date-function support.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use uuid::Uuid;

use crate::artifact::artifact_model::{ResolvedValue, FileReference};
use crate::artifact::source_choice::ShardKey;
use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};
use crate::error::AppError;

const DATA_SUBDIR: &str = "data";

pub fn write_sqlite_shards(
    values: &[ResolvedValue],
    output_dir: &Path,
) -> Result<Vec<FileReference>, AppError> {
    let data_dir: PathBuf = output_dir.join(DATA_SUBDIR);
    fs::create_dir_all(&data_dir)?;

    let groups: BTreeMap<ShardKey, Vec<&ResolvedValue>> = group_values(values);
    let shards: Vec<FileReference> = shard_values(&data_dir, groups)?;
    Ok(shards)
}

fn group_values(resolved: &[ResolvedValue]) -> BTreeMap<ShardKey, Vec<&ResolvedValue>> {
    let mut grouped: BTreeMap<ShardKey, Vec<&ResolvedValue>> = BTreeMap::new();
    for resolved_value in resolved {
        grouped.entry(ShardKey::from_resolved(resolved_value)).or_default().push(resolved_value);
    }
    grouped
}

fn shard_values(data_dir: &PathBuf, grouped: BTreeMap<ShardKey, Vec<&ResolvedValue>>) -> Result<Vec<FileReference>, AppError> {
    let mut shards: Vec<FileReference> = Vec::with_capacity(grouped.len());
    for (shard_key, values) in grouped {
        let shard: FileReference = write_one_shard(&data_dir, shard_key.statistic_kind, shard_key.license_shard_class, &values)?;
        shards.push(shard);
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
        "{}-{}-tmp.{}.sqlite",
        statistic_kind.code(),
        license_shard_class.as_str(),
        tmp_uuid,
    );
    let path: PathBuf = data_dir.join(&filename);

    let mut connection: Connection = Connection::open(&path)?;
    connection.pragma_update(None, "journal_mode", "MEMORY")?;

    create_schema(&connection)?;
    insert_rows(&mut connection, values)?;

    let byte_count: u64 = fs::metadata(&path)?.len();

    Ok(FileReference { path, byte_count })
}

fn create_schema(connection: &Connection) -> Result<(), AppError> {
    connection.execute_batch(
        r#"
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

fn insert_rows(connection: &mut Connection, values: &[&ResolvedValue]) -> Result<(), AppError> {
    let transaction: rusqlite::Transaction = connection.transaction()?;

    {
        let mut statement: rusqlite::Statement = transaction.prepare(
            r#"
            insert into statistic_value
                (region_iso3, region_id, period_start, period_end, value,
                 data_status, data_source_code, data_source_revision)
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )?;

        for resolved_value in values {
            statement.execute(params![
                resolved_value.region_iso3,
                resolved_value.region_id.as_bytes().as_slice(),
                resolved_value.period.start.format("%Y-%m-%d").to_string(),
                resolved_value.period.end.format("%Y-%m-%d").to_string(),
                resolved_value.value,
                resolved_value.data_status.as_str(),
                resolved_value.data_source_kind.code(),
                resolved_value.data_source_revision,
            ])?;
        }
    }

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

        let shards: Vec<FileReference> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        assert_eq!(shards.len(), 3);
        for shard in &shards {
            assert!(shard.path.exists());
            assert!(shard.byte_count > 0);
            let filename: &str = shard.path.file_name().unwrap().to_str().unwrap();
            assert!(filename.contains("-tmp."));
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

        let shards: Vec<FileReference> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        assert_eq!(shards.len(), 1);
        let connection: Connection = Connection::open(&shards[0].path).unwrap();
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

        let shards: Vec<FileReference> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        let connection: Connection = Connection::open(&shards[0].path).unwrap();
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
    fn write_sqlite_shards_uses_correct_filename_format() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<ResolvedValue> = vec![make_merged(
            StatisticKind::Tfr,
            LicenseShardClass::ShareAlike,
            "USA",
            2022,
            1.66,
        )];

        let shards: Vec<FileReference> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        let filename: &str = shards[0].path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("tfr-share_alike-tmp."));
    }
}
