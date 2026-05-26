//! Artifact-builder data model. The artifact pipeline reads candidate values
//! from the canonical store, applies the source-priority merge, emits
//! per-statistic per-license-class SQLite shards plus a FlatGeobuf geometry
//! shard, computes content hashes, and records a manifest. The types here
//! describe each stage's payload.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;

/// One canonical-store row joined with its data_source + statistic + region
/// metadata. Produced by `read_candidate_values`; consumed by
/// `apply_source_priority` which collapses the (region, statistic, period,
/// license_class)-grouped candidates into a single `MergedValue` each.
#[derive(Debug, Clone)]
pub struct CandidateValue {
    pub region_id: Uuid,
    pub region_iso3: String,
    pub statistic_id: Uuid,
    pub statistic_code: String,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: String,
    pub data_source_id: Uuid,
    pub data_source_code: String,
    pub data_source_revision: String,
    pub data_source_preference_rank: i32,
    pub license_class: String,
}

/// One winning row per (region, statistic, period, license_class) cell after
/// the source-priority merge. Grouped by `(statistic_code, license_class)`
/// when emitted to SQLite shards.
#[derive(Debug, Clone)]
pub struct MergedValue {
    pub region_id: Uuid,
    pub region_iso3: String,
    pub statistic_id: Uuid,
    pub statistic_code: String,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: String,
    pub data_source_code: String,
    pub data_source_revision: String,
    pub license_shard_class: LicenseShardClass,
}

/// Coarse license bucket used to split shards. Multiple `data_source.license_class`
/// values fold into one bucket so a client only has to download the buckets
/// matching its license posture (e.g. a non-commercial app skips `non_commercial`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LicenseShardClass {
    Base,
    ShareAlike,
    NonCommercial,
}

impl LicenseShardClass {
    pub fn from_license_class(license_class: &str) -> LicenseShardClass {
        match license_class {
            "public_domain" | "attribution" => LicenseShardClass::Base,
            "attribution_sa" => LicenseShardClass::ShareAlike,
            "noncommercial" => LicenseShardClass::NonCommercial,
            _ => LicenseShardClass::Base,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LicenseShardClass::Base => "base",
            LicenseShardClass::ShareAlike => "share_alike",
            LicenseShardClass::NonCommercial => "non_commercial",
        }
    }
}

/// A single output file produced by a writer phase. Path is the on-disk
/// location; `byte_count` is captured at write time so later phases (manifest)
/// don't have to re-stat.
#[derive(Debug, Clone)]
pub struct ShardOutput {
    pub path: PathBuf,
    pub byte_count: u64,
}

/// Bundle of all hashed outputs produced by `compute_content_hashes`. Each
/// `(statistic_code, license_shard_class) -> ShardOutput` plus the geometry
/// shard. Manifest emission consumes this.
#[derive(Debug, Clone)]
pub struct HashedOutputs {
    pub statistic_shards: Vec<HashedStatisticShard>,
    pub geometry_shard: HashedShard,
}

#[derive(Debug, Clone)]
pub struct HashedStatisticShard {
    pub statistic_code: String,
    pub license_shard_class: LicenseShardClass,
    pub shard: HashedShard,
}

#[derive(Debug, Clone)]
pub struct HashedShard {
    pub path: PathBuf,
    pub byte_count: u64,
    pub sha256_hex: String,
}

/// End-state of `build_artifacts` against a local directory. Every file lives
/// on disk under `output_dir`; nothing is uploaded yet. `publish_artifacts`
/// consumes a `LocalArtifactBuild` by reference.
#[derive(Debug, Clone)]
pub struct LocalArtifactBuild {
    pub output_dir: PathBuf,
    pub version_label: String,
    pub hashed: HashedOutputs,
    pub manifest: HashedShard,
}

/// Row in the `artifact_version` table, inserted by `publish_artifacts` after
/// every file has been put successfully.
#[derive(Debug, Clone)]
pub struct ArtifactVersion {
    pub id: Uuid,
    pub version_label: String,
    pub artifact_created: DateTime<Utc>,
    pub manifest_sha256: String,
    pub manifest_url: String,
    pub data_source_versions_jsonb: serde_json::Value,
    pub notes: Option<String>,
}
