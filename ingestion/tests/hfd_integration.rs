//! Integration tests for the HFD adapter pipeline against `eafora_test`. Each test opens its own
//! transaction and rolls back at teardown, so Postgres MVCC isolates them and they run in parallel.

mod helpers;

use chrono::{NaiveDate, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use ingestion::adapter::{IngestWarning, IngestWarningKind, NormalizedStatisticValue};
use ingestion::canonical::canonical_entity::StatisticValue;
use ingestion::hfd::hfd_adapter;
use ingestion::hfd::hfd_client;
use ingestion::hfd::hfd_model::{ParsedHfdPublication, ParsedHfdStatisticValue};
use ingestion::ingest;
use ingestion::ingest::IngestReport;
use shared::canonical::canonical_model::{DataSourceKind, DataStatus, NaiveDatePeriod, StatisticKind};

use helpers::canonical::{get_country_region_id, get_data_source_id, get_statistic_id};

const SAMPLE_COHORT_FILE: &str = include_str!("../samples/hfd/tfrVH.txt");

fn parse_sample() -> (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) {
    hfd_client::parse_fertility_file(SAMPLE_COHORT_FILE, hfd_client::COHORT_FERTILITY_COLUMNS)
        .expect("the sample parses")
}

async fn normalize_sample(
    transaction: &mut Transaction<'static, Postgres>,
) -> (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) {
    let (_, parsed_hfd_statistic_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) = parse_sample();

    hfd_adapter::normalize(&mut **transaction, parsed_hfd_statistic_values, StatisticKind::Ccf)
        .await
        .expect("normalize succeeds")
}

async fn record_sample(
    transaction: &mut Transaction<'static, Postgres>,
    revision_label: &str,
) -> IngestReport {
    let (normalized_statistic_values, _): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize_sample(transaction).await;
    let data_source_id: Uuid = get_data_source_id(transaction, DataSourceKind::HumanFertilityDatabase).await;

    ingest::record_statistic_values(
        &mut **transaction,
        data_source_id,
        revision_label,
        None,
        Utc::now(),
        normalized_statistic_values,
    )
    .await
    .expect("record_statistic_values succeeds")
}

#[tokio::test]
async fn normalize_maps_a_bare_alpha3_code_to_its_region() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let (normalized_statistic_values, _): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize_sample(&mut transaction).await;
    let austria_region_id: Uuid = get_country_region_id(&mut transaction, "AUT").await;

    let austrian_values: Vec<&NormalizedStatisticValue> = normalized_statistic_values
        .iter()
        .filter(|statistic_value| statistic_value.region_id == austria_region_id)
        .collect();

    assert_eq!(austrian_values.len(), 3);

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_maps_a_national_total_code_to_its_country() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let (normalized_statistic_values, _): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize_sample(&mut transaction).await;
    let germany_region_id: Uuid = get_country_region_id(&mut transaction, "DEU").await;

    let german_values: Vec<&NormalizedStatisticValue> = normalized_statistic_values
        .iter()
        .filter(|statistic_value| statistic_value.region_id == germany_region_id)
        .collect();

    assert_eq!(german_values.len(), 1);
    assert_eq!(german_values[0].value, 1.916);

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_warns_for_a_subnational_territory_and_writes_nothing() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let (_, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize_sample(&mut transaction).await;

    let territory_warnings: Vec<&str> = warnings
        .iter()
        .filter(|warning| warning.kind == IngestWarningKind::UnrecognizedRegionCode)
        .map(|warning| warning.message.as_str())
        .collect();

    assert_eq!(territory_warnings.len(), 2);
    assert!(territory_warnings.iter().any(|message| message.contains("DEUTE")));
    assert!(territory_warnings.iter().any(|message| message.contains("GBR_SCO")));

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_warns_once_for_a_region_that_yielded_no_values() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let (_, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize_sample(&mut transaction).await;

    let no_value_warnings: Vec<&str> = warnings
        .iter()
        .filter(|warning| warning.kind == IngestWarningKind::NoValuesForRegion)
        .map(|warning| warning.message.as_str())
        .collect();

    assert_eq!(no_value_warnings.len(), 1);
    assert!(no_value_warnings[0].contains("CHL"));

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_warns_for_a_bare_code_with_no_canonical_region() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let unknown: Vec<ParsedHfdStatisticValue> = vec![ParsedHfdStatisticValue {
        hfd_code: "ZZZ".to_string(),
        period_year: 1950,
        value: Some(2.1),
    }];

    let (normalized_statistic_values, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        hfd_adapter::normalize(&mut *transaction, unknown, StatisticKind::Ccf)
            .await
            .expect("normalize succeeds rather than stopping the run");

    assert!(normalized_statistic_values.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, IngestWarningKind::UnrecognizedRegionCode);
    assert!(warnings[0].message.contains("ZZZ"));

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_drops_an_absent_value_without_a_warning() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let (normalized_statistic_values, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize_sample(&mut transaction).await;

    assert_eq!(normalized_statistic_values.len(), 6);
    assert!(warnings
        .iter()
        .all(|warning| warning.kind != IngestWarningKind::NotApplicableValue));

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_encodes_a_cohort_as_a_one_year_period() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let (normalized_statistic_values, _): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize_sample(&mut transaction).await;
    let austria_region_id: Uuid = get_country_region_id(&mut transaction, "AUT").await;

    let earliest_austrian: &NormalizedStatisticValue = normalized_statistic_values
        .iter()
        .filter(|statistic_value| statistic_value.region_id == austria_region_id)
        .min_by_key(|statistic_value| statistic_value.period.start)
        .expect("austria has values");

    assert_eq!(earliest_austrian.period.start.to_string(), "1936-01-01");
    assert_eq!(earliest_austrian.period.end.to_string(), "1937-01-01");

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_attributes_every_value_to_completed_cohort_fertility() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let (normalized_statistic_values, _): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize_sample(&mut transaction).await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, StatisticKind::Ccf.code()).await;

    assert!(!normalized_statistic_values.is_empty());
    assert!(normalized_statistic_values
        .iter()
        .all(|statistic_value| statistic_value.statistic_id == statistic_id));

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn record_statistic_values_adds_every_value_on_a_first_run() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let report: IngestReport = record_sample(&mut transaction, "2026-07-02").await;

    assert_eq!(report.values_added, 6);
    assert_eq!(report.values_revised, 0);
    assert_eq!(report.values_skipped, 0);

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn record_statistic_values_skips_every_value_on_an_unchanged_second_run() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    record_sample(&mut transaction, "2026-07-02").await;
    let second: IngestReport = record_sample(&mut transaction, "2026-07-02").await;

    assert_eq!(second.values_added, 0);
    assert_eq!(second.values_revised, 0);
    assert_eq!(second.values_skipped, 6);

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn record_statistic_values_supersedes_a_revised_value_and_keeps_the_original() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    record_sample(&mut transaction, "2026-07-02").await;

    let austria_region_id: Uuid = get_country_region_id(&mut transaction, "AUT").await;
    let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::HumanFertilityDatabase).await;

    let revised: Vec<ParsedHfdStatisticValue> = vec![ParsedHfdStatisticValue {
        hfd_code: "AUT".to_string(),
        period_year: 1936,
        value: Some(2.500),
    }];
    let (revised_statistic_values, _): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        hfd_adapter::normalize(&mut *transaction, revised, StatisticKind::Ccf)
            .await
            .expect("normalize succeeds");
    let revised_statistic_value: NormalizedStatisticValue = revised_statistic_values
        .into_iter()
        .next()
        .expect("the revised value normalized");
    let report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "2026-08-01",
        None,
        Utc::now(),
        vec![revised_statistic_value],
    )
    .await
    .expect("record_statistic_values succeeds");

    assert_eq!(report.values_revised, 1);

    let probe: NormalizedStatisticValue = NormalizedStatisticValue {
        region_id: austria_region_id,
        statistic_id: get_statistic_id(&mut transaction, StatisticKind::Ccf.code()).await,
        period: NaiveDatePeriod::from_year(1936).unwrap(),
        value: 0.0,
        data_status: DataStatus::Final,
    };
    let current: Option<StatisticValue> =
        ingest::ingest_db::find_current_value(&mut *transaction, &probe, data_source_id)
            .await
            .expect("find_current_value succeeds");

    assert_eq!(current.expect("a current value exists").value, 2.500);

    let recorded_count: i64 = sqlx::query_scalar!(
        "select count(*) as \"count!\" from statistic_value where region_id = $1 and period_start = $2",
        austria_region_id,
        NaiveDate::from_ymd_opt(1936, 1, 1).unwrap(),
    )
    .fetch_one(&mut *transaction)
    .await
    .expect("count the recorded values");

    assert_eq!(recorded_count, 2);

    transaction.rollback().await.unwrap();
}
