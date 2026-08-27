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

use crate::artifact::artifact_model::{PartitionedValue, StatisticShard};
use crate::error::AppError;
use crate::artifact::compression::PlainArtifact;

pub fn write_sqlite_shards(
    values: &[PartitionedValue],
    data_dir: &Path,
) -> Result<Vec<StatisticShard<PlainArtifact>>, AppError> {
    fs::create_dir_all(data_dir)?;

    let groups: BTreeMap<StatisticShardKey, Vec<&PartitionedValue>> = group_values(values);
    let shards: Vec<StatisticShard<PlainArtifact>> = shard_values(data_dir, groups)?;
    Ok(shards)
}

fn group_values(values: &[PartitionedValue]) -> BTreeMap<StatisticShardKey, Vec<&PartitionedValue>> {
    let mut grouped: BTreeMap<StatisticShardKey, Vec<&PartitionedValue>> = BTreeMap::new();
    for partitioned_value in values {
        grouped.entry(partitioned_value.shard_key()).or_default().push(partitioned_value);
    }
    grouped
}

fn shard_values(data_dir: &Path, grouped: BTreeMap<StatisticShardKey, Vec<&PartitionedValue>>) -> Result<Vec<StatisticShard<PlainArtifact>>, AppError> {
    let mut shards: Vec<StatisticShard<PlainArtifact>> = Vec::with_capacity(grouped.len());
    for (shard_key, values) in grouped {
        let file: FileReference = write_one_shard(data_dir, shard_key.statistic_kind, shard_key.license_shard_class, &values)?;
        shards.push(StatisticShard {
            key: shard_key,
            file: PlainArtifact { file },
        });
    }
    Ok(shards)
}

fn write_one_shard(
    data_dir: &Path,
    statistic_kind: StatisticKind,
    license_shard_class: LicenseShardClass,
    values: &[&PartitionedValue],
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
    insert_data_sources(&connection, values)?;
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

/// Each source's rank travels with the shard so a consumer resolves between sources without the manifest.
fn insert_data_sources(connection: &Connection, values: &[&PartitionedValue]) -> Result<(), AppError> {
    let mut preference_rank_by_source: BTreeMap<&str, i32> = BTreeMap::new();

    for partitioned_value in values {
        preference_rank_by_source.insert(
            partitioned_value.data_source_kind.code(),
            partitioned_value.data_source_preference_rank,
        );
    }

    for (source_code, preference_rank) in preference_rank_by_source {
        connection.execute(
            formatcp!(
                "insert into {} ({}, {}) values (?1, ?2)",
                schema::TABLE_DATA_SOURCE, schema::COL_DATA_SOURCE_CODE, schema::COL_PREFERENCE_RANK,
            ),
            (source_code, preference_rank),
        )?;
    }

    Ok(())
}

fn insert_rows(connection: &mut Connection, values: &[&PartitionedValue]) -> Result<(), AppError> {
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

    for partitioned_value in values {
        statement.execute((
            &partitioned_value.region_code,
            partitioned_value.region_id.as_bytes().as_slice(),
            partitioned_value.period.start.format(schema::PERIOD_DATE_FORMAT).to_string(),
            partitioned_value.period.end.format(schema::PERIOD_DATE_FORMAT).to_string(),
            partitioned_value.value,
            partitioned_value.data_status.as_str(),
            partitioned_value.data_source_kind.code(),
            &partitioned_value.data_source_revision,
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
    ) -> PartitionedValue {
        make_sourced(
            statistic_kind,
            license_shard_class,
            region_code,
            year,
            value,
            DataSourceKind::WorldBankWDI,
            100,
        )
    }

    fn make_sourced(
        statistic_kind: StatisticKind,
        license_shard_class: LicenseShardClass,
        region_code: &str,
        year: i32,
        value: f64,
        data_source_kind: DataSourceKind,
        data_source_preference_rank: i32,
    ) -> PartitionedValue {
        PartitionedValue {
            region_id: Uuid::from_u128(year as u128),
            region_code: region_code.to_string(),
            statistic_kind,
            period: NaiveDatePeriod::from_year(year).unwrap(),
            value,
            data_status: DataStatus::Final,
            data_source_kind,
            data_source_preference_rank,
            data_source_revision: "2024-Q4".to_string(),
            license_shard_class,
        }
    }

    /// Both sources' values for one cell reach the shard, with each source's rank, so the consumer can
    /// resolve and offer the alternative.
    #[test]
    fn write_sqlite_shards_keeps_every_source_for_a_contested_cell() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<PartitionedValue> = vec![
            make_sourced(StatisticKind::Tfr, LicenseShardClass::Base, "deu", 2016, 1.6, DataSourceKind::WorldBankWDI, 100),
            make_sourced(StatisticKind::Tfr, LicenseShardClass::Base, "deu", 2016, 1.597, DataSourceKind::HumanFertilityDatabase, 50),
        ];

        let shards: Vec<StatisticShard<PlainArtifact>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        assert_eq!(shards.len(), 1);

        let connection: Connection = Connection::open(&shards[0].file.file.path).unwrap();
        let value_count: i64 = connection
            .query_row(&format!("select count(*) from {}", schema::TABLE_STATISTIC_VALUE), [], |row| row.get(0))
            .unwrap();
        let ranks: Vec<(String, i32)> = connection
            .prepare(&format!(
                "select {}, {} from {} order by {} asc",
                schema::COL_DATA_SOURCE_CODE, schema::COL_PREFERENCE_RANK,
                schema::TABLE_DATA_SOURCE, schema::COL_PREFERENCE_RANK,
            ))
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();

        assert_eq!(value_count, 2);
        assert_eq!(ranks, vec![("hfd".to_string(), 50), ("wb_wdi".to_string(), 100)]);
    }

    #[test]
    fn write_sqlite_shards_creates_one_file_per_statistic_per_license_class() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<PartitionedValue> = vec![
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "usa", 2022, 1.66),
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "jpn", 2022, 1.30),
            make_merged(StatisticKind::Tfr, LicenseShardClass::NonCommercial, "usa", 2022, 1.66),
            make_merged(StatisticKind::Ccf, LicenseShardClass::Base, "usa", 2022, 1.85),
        ];

        let shards: Vec<StatisticShard<PlainArtifact>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        assert_eq!(shards.len(), 3);
        for shard in &shards {
            assert!(shard.file.file.path.exists());
            assert!(shard.file.file.byte_count > 0);
            let filename: &str = shard.file.file.path.file_name().unwrap().to_str().unwrap();
            assert!(filename.contains(".tmp-"));
            assert!(filename.ends_with(".sqlite"));
        }
    }

    #[test]
    fn write_sqlite_shards_writes_rows_with_expected_schema() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<PartitionedValue> = vec![
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "usa", 2022, 1.66),
            make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "jpn", 2022, 1.30),
        ];

        let shards: Vec<StatisticShard<PlainArtifact>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        assert_eq!(shards.len(), 1);
        let connection: Connection = Connection::open(&shards[0].file.file.path).unwrap();
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
        let merged: Vec<PartitionedValue> = vec![make_merged(StatisticKind::Tfr, LicenseShardClass::Base, "usa", 2022, 1.66)];

        let shards: Vec<StatisticShard<PlainArtifact>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        let connection: Connection = Connection::open(&shards[0].file.file.path).unwrap();
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
        let merged: Vec<PartitionedValue> = vec![make_merged(
            StatisticKind::Tfr,
            LicenseShardClass::NonCommercial,
            "usa",
            2022,
            1.66,
        )];

        let shards: Vec<StatisticShard<PlainArtifact>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();
        let connection: Connection = Connection::open(&shards[0].file.file.path).unwrap();

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
        let merged: Vec<PartitionedValue> = vec![make_merged(
            StatisticKind::Tfr,
            LicenseShardClass::ShareAlike,
            "usa",
            2022,
            1.66,
        )];

        let shards: Vec<StatisticShard<PlainArtifact>> = write_sqlite_shards(&merged, temp_dir.path()).unwrap();

        let filename: &str = shards[0].file.file.path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("tfr-share_alike.tmp-"));
    }
}
