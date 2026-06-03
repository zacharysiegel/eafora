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

use crate::artifact::artifact_model::{MergedValue, ShardOutput};
use crate::canonical::canonical_model::LicenseShardClass;
use crate::error::AppError;

const DATA_SUBDIR: &str = "data";

pub fn emit_sqlite_shards(
    merged: &[MergedValue],
    output_dir: &Path,
) -> Result<Vec<ShardOutput>, AppError> {
    let data_dir: PathBuf = output_dir.join(DATA_SUBDIR);
    fs::create_dir_all(&data_dir)?;

    let mut grouped: BTreeMap<(String, LicenseShardClass), Vec<&MergedValue>> = BTreeMap::new();
    for merged_value in merged {
        let key: (String, LicenseShardClass) = (
            merged_value.statistic_code.clone(),
            merged_value.license_shard_class,
        );
        grouped.entry(key).or_default().push(merged_value);
    }

    let mut shards: Vec<ShardOutput> = Vec::with_capacity(grouped.len());
    for ((statistic_code, license_shard_class), values) in grouped {
        let shard: ShardOutput = write_one_shard(&data_dir, &statistic_code, license_shard_class, &values)?;
        shards.push(shard);
    }

    Ok(shards)
}

fn write_one_shard(
    data_dir: &Path,
    statistic_code: &str,
    license_shard_class: LicenseShardClass,
    values: &[&MergedValue],
) -> Result<ShardOutput, AppError> {
    let tmp_uuid: Uuid = Uuid::now_v7();
    let filename: String = format!(
        "{}-{}-tmp.{}.sqlite",
        statistic_code,
        license_shard_class.as_str(),
        tmp_uuid,
    );
    let path: PathBuf = data_dir.join(&filename);

    let mut connection: Connection = Connection::open(&path)?;
    connection.pragma_update(None, "journal_mode", "MEMORY")?;

    create_schema(&connection)?;
    insert_rows(&mut connection, values)?;

    let byte_count: u64 = fs::metadata(&path)?.len();

    Ok(ShardOutput { path, byte_count })
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

fn insert_rows(connection: &mut Connection, values: &[&MergedValue]) -> Result<(), AppError> {
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

        for merged_value in values {
            statement.execute(params![
                merged_value.region_iso3,
                merged_value.region_id.as_bytes().as_slice(),
                merged_value.period.start.format("%Y-%m-%d").to_string(),
                merged_value.period.end.format("%Y-%m-%d").to_string(),
                merged_value.value,
                merged_value.data_status.as_str(),
                merged_value.data_source_kind.code(),
                merged_value.data_source_revision,
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
        statistic_code: &str,
        license_shard_class: LicenseShardClass,
        region_iso3: &str,
        year: i32,
        value: f64,
    ) -> MergedValue {
        MergedValue {
            region_id: Uuid::from_u128(year as u128),
            region_iso3: region_iso3.to_string(),
            statistic_id: Uuid::from_u128(1),
            statistic_code: statistic_code.to_string(),
            period: NaiveDatePeriod::from_year(year).unwrap(),
            value,
            data_status: DataStatus::Final,
            data_source_kind: DataSourceKind::WorldBankWDI,
            data_source_revision: "2024-Q4".to_string(),
            license_shard_class,
        }
    }

    #[test]
    fn emit_sqlite_shards_creates_one_file_per_statistic_per_license_class() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<MergedValue> = vec![
            make_merged("tfr", LicenseShardClass::Base, "USA", 2022, 1.66),
            make_merged("tfr", LicenseShardClass::Base, "JPN", 2022, 1.30),
            make_merged("tfr", LicenseShardClass::NonCommercial, "USA", 2022, 1.66),
            make_merged("ctfr", LicenseShardClass::Base, "USA", 2022, 1.85),
        ];

        let shards: Vec<ShardOutput> = emit_sqlite_shards(&merged, temp_dir.path()).unwrap();

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
    fn emit_sqlite_shards_writes_rows_with_expected_schema() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<MergedValue> = vec![
            make_merged("tfr", LicenseShardClass::Base, "USA", 2022, 1.66),
            make_merged("tfr", LicenseShardClass::Base, "JPN", 2022, 1.30),
        ];

        let shards: Vec<ShardOutput> = emit_sqlite_shards(&merged, temp_dir.path()).unwrap();

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
    fn emit_sqlite_shards_index_is_present() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<MergedValue> = vec![make_merged("tfr", LicenseShardClass::Base, "USA", 2022, 1.66)];

        let shards: Vec<ShardOutput> = emit_sqlite_shards(&merged, temp_dir.path()).unwrap();

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
    fn emit_sqlite_shards_uses_correct_filename_format() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let merged: Vec<MergedValue> = vec![make_merged(
            "tfr",
            LicenseShardClass::ShareAlike,
            "USA",
            2022,
            1.66,
        )];

        let shards: Vec<ShardOutput> = emit_sqlite_shards(&merged, temp_dir.path()).unwrap();

        let filename: &str = shards[0].path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("tfr-share_alike-tmp."));
    }
}
