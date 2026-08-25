use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use shared::artifact::bundle::StatisticShardKey;
use shared::canonical::canonical_model::{
    DataSourceKind, DataStatus, LicenseClass, LicenseShardClass, NaiveDatePeriod, SourceAttribution, SourceRevision,
    StatisticDefinition, StatisticKind,
};
use shared::filesystem::{FileReference, Hashed};

use crate::error::AppError;

/// One value as it sits in the canonical store: a single source's
/// reading for a `(region, statistic, period)` cell. Multiple candidates
/// can exist for the same cell, one per data source that publishes it.
/// Carries the source's `license_class`; the shard bin isn't decided yet.
#[derive(Debug, Clone)]
pub struct CandidateValue {
    pub region_id: Uuid,
    pub region_code: String,
    pub statistic_kind: StatisticKind,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: DataStatus,
    pub data_source_kind: DataSourceKind,
    pub data_source_preference_rank: i32,
    pub data_source_revision: String,
    pub license_class: LicenseClass,
}

#[derive(Debug, Clone)]
pub struct CandidateValueProjection {
    pub region_id: Uuid,
    pub region_code: String,
    pub statistic_code: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub value: f64,
    pub data_status: String,
    pub data_source_code: String,
    pub data_source_preference_rank: i32,
    pub data_source_revision: String,
    pub license_class: String,
}

impl TryFrom<CandidateValueProjection> for CandidateValue {
    type Error = AppError;

    fn try_from(projection: CandidateValueProjection) -> Result<Self, Self::Error> {
        Ok(CandidateValue {
            region_id: projection.region_id,
            region_code: projection.region_code,
            statistic_kind: StatisticKind::try_from(projection.statistic_code.as_str())?,
            period: NaiveDatePeriod {
                start: projection.period_start,
                end: projection.period_end,
            },
            value: projection.value,
            data_status: DataStatus::try_from(projection.data_status.as_str())?,
            data_source_kind: DataSourceKind::try_from(projection.data_source_code.as_str())?,
            data_source_preference_rank: projection.data_source_preference_rank,
            data_source_revision: projection.data_source_revision,
            license_class: LicenseClass::try_from(projection.license_class.as_str())?,
        })
    }
}

/// Per-country attributes read from `country`/`region` for geometry writing: the `Country.iso3` (used
/// to match a Natural Earth `ADM0_A3` feature to its seeded country), the English name, and the
/// `region.code` slug written as the feature's join key.
#[derive(Debug, Clone)]
pub struct CountryMetadataProjection {
    pub iso3: String,
    pub name_en: String,
    pub region_code: String,
}

/// A candidate assigned to the shard its licence puts it in.
#[derive(Debug, Clone)]
pub struct PartitionedValue {
    pub region_id: Uuid,
    pub region_code: String,
    pub statistic_kind: StatisticKind,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: DataStatus,
    pub data_source_kind: DataSourceKind,
    pub data_source_preference_rank: i32,
    pub data_source_revision: String,
    pub license_shard_class: LicenseShardClass,
}

impl PartitionedValue {
    pub fn from_candidate(candidate: &CandidateValue, license_shard_class: LicenseShardClass) -> Self {
        PartitionedValue {
            region_id: candidate.region_id,
            region_code: candidate.region_code.clone(),
            statistic_kind: candidate.statistic_kind,
            period: candidate.period,
            value: candidate.value,
            data_status: candidate.data_status,
            data_source_kind: candidate.data_source_kind,
            data_source_preference_rank: candidate.data_source_preference_rank,
            data_source_revision: candidate.data_source_revision.clone(),
            license_shard_class,
        }
    }

    pub fn shard_key(&self) -> StatisticShardKey {
        StatisticShardKey {
            statistic_kind: self.statistic_kind,
            license_shard_class: self.license_shard_class,
        }
    }
}

/// What one pass over a bundle's data sources yields: the revision each one is at, and the attribution a
/// consumer must display for it.
#[derive(Debug, Clone)]
pub struct SourceDetail {
    pub revisions: BTreeMap<DataSourceKind, SourceRevision>,
    pub attribution: BTreeMap<DataSourceKind, SourceAttribution>,
}

/// Everything a manifest says about the data rather than about the files: where each source stood when the
/// bundle was built, and the text a consumer must show for each source and statistic.
#[derive(Debug, Clone)]
pub struct BundleProvenance {
    pub source_revisions: BTreeMap<DataSourceKind, SourceRevision>,
    pub source_attribution: BTreeMap<DataSourceKind, SourceAttribution>,
    pub statistic_definitions: BTreeMap<StatisticKind, StatisticDefinition>,
}

#[derive(Debug, Clone)]
pub struct StatisticShard<F> {
    pub key: StatisticShardKey,
    pub file: F,
}

#[derive(Debug, Clone)]
pub struct Artifacts {
    pub shards: Vec<StatisticShard<Hashed<FileReference>>>,
    pub geometry: Hashed<FileReference>,
    pub manifest: Hashed<FileReference>,
}

#[derive(Debug, Clone)]
pub struct BuildReport {
    pub artifact_dir: PathBuf,
    pub version_label: String,
    pub artifacts: Artifacts,
    pub data_source_revisions: BTreeMap<DataSourceKind, SourceRevision>,
}

/// The pair of bundles one build emits for a single version: the complete bundle (all periods and
/// sources, published to the CDN) and the downsampled bundle (World Bank WDI at the United States
/// reference year, embedded into clients). Each is a self-contained tree under the version directory.
#[derive(Debug, Clone)]
pub struct CoupledBuildReport {
    pub complete: BuildReport,
    pub downsampled: BuildReport,
}

#[derive(Debug, Clone)]
pub struct ArtifactVersion {
    pub id: Uuid,
    pub version_label: String,
    pub artifact_created: DateTime<Utc>,
    pub manifest_sha256: String,
    pub manifest_url: String,
    pub data_source_revisions_jsonb: serde_json::Value,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtifactVersionEntity {
    pub id: Uuid,
    pub version_label: String,
    pub artifact_created: DateTime<Utc>,
    pub manifest_sha256: String,
    pub manifest_url: String,
    pub data_source_revisions_jsonb: serde_json::Value,
    pub notes: Option<String>,
}

impl From<ArtifactVersionEntity> for ArtifactVersion {
    fn from(entity: ArtifactVersionEntity) -> Self {
        ArtifactVersion {
            id: entity.id,
            version_label: entity.version_label,
            artifact_created: entity.artifact_created,
            manifest_sha256: entity.manifest_sha256,
            manifest_url: entity.manifest_url,
            data_source_revisions_jsonb: entity.data_source_revisions_jsonb,
            notes: entity.notes,
        }
    }
}
