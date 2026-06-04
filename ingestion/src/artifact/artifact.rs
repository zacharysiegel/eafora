use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use sqlx::{PgConnection, PgExecutor};

use crate::artifact::{artifact_db, content_hashing, source_choice};
use crate::artifact::artifact_model::{
    CandidateValue, HashedOutputs, HashedShard, LocalArtifactBuild, ResolvedValue, ShardOutput,
};
use crate::artifact::writer::{flatgeobuf, manifest, sqlite};
use crate::artifact::writer::manifest::ManifestEmission;
use crate::canonical::canonical_db;
use crate::canonical::canonical_model::{DataSourceKind, SourceChoice, SourceRevision, StatisticKind};
use crate::error::AppError;
use crate::ingest::ingest_db;

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    pub test_offline: bool,
}

pub async fn build_artifacts(
    connection: &mut PgConnection,
    output_dir: &Path,
    version_label: &str,
    options: BuildOptions,
) -> Result<LocalArtifactBuild, AppError> {
    let started: Instant = Instant::now();
    log::info!(
        "build_artifacts: starting version_label={} output_dir={:?}",
        version_label, output_dir,
    );

    fs::create_dir_all(output_dir)?;

    let candidate_values: Vec<CandidateValue> = artifact_db::read_candidate_values(&mut *connection).await?;
    log::info!("build_artifacts: read {} candidate values", candidate_values.len());

    warn_on_statistics_without_values(&mut *connection, &candidate_values).await?;

    let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> =
        read_data_source_revisions(&mut *connection, &candidate_values).await?;
    let source_choices: Vec<SourceChoice> = canonical_db::read_source_choices(&mut *connection).await?;
    let resolved_values: Vec<ResolvedValue> = source_choice::resolve_candidates(candidate_values, &source_choices)?;
    log::info!("build_artifacts: merged into {} values", resolved_values.len());

    let sqlite_shards: Vec<ShardOutput> = sqlite::emit_sqlite_shards(&resolved_values, output_dir)?;
    log::info!("build_artifacts: emitted {} sqlite shards", sqlite_shards.len());

    let geometry_shard: ShardOutput = if options.test_offline {
        flatgeobuf::emit_placeholder_geometry(output_dir)?
    } else {
        flatgeobuf::emit_geometry_flatgeobuf(&mut *connection, output_dir).await?
    };
    log::info!("build_artifacts: emitted geometry shard {:?}", geometry_shard.path);

    let hashed: HashedOutputs = content_hashing::compute_content_hashes(sqlite_shards, geometry_shard)?;

    let manifest_emission: ManifestEmission =
        manifest::emit_manifest(&hashed, version_label, &data_source_revisions, output_dir)?;
    let manifest: HashedShard = HashedShard {
        path: manifest_emission.output.path,
        byte_count: manifest_emission.output.byte_count,
        sha256_hex: manifest_emission.sha256_hex,
    };

    log::info!(
        "build_artifacts: complete in {:?}; manifest sha256={}",
        started.elapsed(), manifest.sha256_hex,
    );

    Ok(LocalArtifactBuild {
        output_dir: output_dir.to_path_buf(),
        version_label: version_label.to_string(),
        hashed,
        manifest,
    })
}

async fn read_data_source_revisions(
    connection: &mut PgConnection,
    candidate_values: &[CandidateValue],
) -> Result<BTreeMap<DataSourceKind, SourceRevision>, AppError> {
    let kinds: BTreeSet<DataSourceKind> = candidate_values.iter().map(|candidate| candidate.data_source_kind).collect();

    let mut revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
    for kind in kinds {
        let data_source = canonical_db::find_data_source_by_kind(&mut *connection, kind)
            .await?
            .ok_or_else(|| AppError::from(format!("read_data_source_revisions: data_source {:?} missing from canonical store", kind)))?;
        let revision = ingest_db::read_latest_publication(&mut *connection, data_source.id)
            .await?
            .ok_or_else(|| AppError::from(format!("read_data_source_revisions: no publication recorded for {:?}", kind)))?;
        revisions.insert(kind, revision);
    }

    Ok(revisions)
}

async fn warn_on_statistics_without_values<'e>(
    executor: impl PgExecutor<'e>,
    candidates: &[CandidateValue],
) -> Result<(), AppError> {
    let known_kinds: Vec<StatisticKind> = artifact_db::read_all_statistic_kinds(executor).await?;
    let kinds_with_values: BTreeSet<StatisticKind> = candidates
        .iter()
        .map(|candidate| candidate.statistic_kind)
        .collect();

    for kind in &known_kinds {
        if !kinds_with_values.contains(kind) {
            log::warn!(
                "build_artifacts: statistic {:?} has no candidate values; shard will be missing from this build",
                kind,
            );
        }
    }
    Ok(())
}
