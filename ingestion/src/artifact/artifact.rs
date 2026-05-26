//! End-to-end orchestrator for `build`. Reads candidate values from the
//! canonical store, applies the source-priority merge, emits per-statistic
//! per-license-class SQLite shards plus a FlatGeobuf geometry shard,
//! content-hashes everything, writes manifest.json. Returns a
//! `LocalArtifactBuild` describing the on-disk state; nothing is uploaded.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::artifact::artifact_db::{read_all_statistic_codes, read_candidate_values};
use crate::artifact::artifact_model::{
    CandidateValue, HashedOutputs, HashedShard, LocalArtifactBuild, MergedValue, ShardOutput,
};
use crate::artifact::content_hashing::compute_content_hashes;
use crate::artifact::source_priority::{apply_source_priority, collect_data_source_versions};
use crate::artifact::writer::flatgeobuf::emit_geometry_flatgeobuf;
use crate::artifact::writer::manifest::{emit_manifest, ManifestEmission};
use crate::artifact::writer::sqlite::emit_sqlite_shards;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    /// If true, skip the Natural Earth download and emit a stub geometry
    /// shard instead. Used by tests that exercise the orchestrator without
    /// live HTTP. The stub is hashed + renamed like a real shard, so the
    /// manifest stays well-formed.
    pub use_placeholder_geometry: bool,
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

    std::fs::create_dir_all(output_dir)
        .map_err(|err| AppError::from(format!("build_artifacts: create_dir {:?}: {}", output_dir, err)))?;

    let candidate_values: Vec<CandidateValue> = read_candidate_values(&mut *connection).await?;
    log::info!("build_artifacts: read {} candidate values", candidate_values.len());

    warn_on_statistics_without_values(&mut *connection, &candidate_values).await?;

    let data_source_versions: BTreeMap<String, String> = collect_data_source_versions(&candidate_values);
    let merged_values: Vec<MergedValue> = apply_source_priority(candidate_values);
    log::info!("build_artifacts: merged into {} values", merged_values.len());

    let sqlite_shards: Vec<ShardOutput> = emit_sqlite_shards(&merged_values, output_dir)?;
    log::info!("build_artifacts: emitted {} sqlite shards", sqlite_shards.len());

    let geometry_shard: ShardOutput = if options.use_placeholder_geometry {
        write_placeholder_geometry(output_dir)?
    } else {
        emit_geometry_flatgeobuf(&mut *connection, output_dir).await?
    };
    log::info!("build_artifacts: emitted geometry shard {:?}", geometry_shard.path);

    let hashed: HashedOutputs = compute_content_hashes(sqlite_shards, geometry_shard)?;

    let manifest_emission: ManifestEmission =
        emit_manifest(&hashed, version_label, &data_source_versions, output_dir)?;
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

async fn warn_on_statistics_without_values<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    candidates: &[CandidateValue],
) -> Result<(), AppError> {
    let known_codes: Vec<String> = read_all_statistic_codes(executor).await?;
    let codes_with_values: BTreeSet<String> = candidates
        .iter()
        .map(|candidate| candidate.statistic_code.clone())
        .collect();

    for code in &known_codes {
        if !codes_with_values.contains(code) {
            log::warn!(
                "build_artifacts: statistic {} has no candidate values; shard will be missing from this build",
                code,
            );
        }
    }
    Ok(())
}

fn write_placeholder_geometry(output_dir: &Path) -> Result<ShardOutput, AppError> {
    let geometry_dir: PathBuf = output_dir.join("geometry");
    std::fs::create_dir_all(&geometry_dir)
        .map_err(|err| AppError::from(format!("build_artifacts: create geometry dir: {}", err)))?;

    let placeholder_path: PathBuf = geometry_dir.join(format!("world-50m-tmp.{}.fgb", Uuid::now_v7()));
    let placeholder_bytes: &[u8] = b"FGB-PLACEHOLDER";
    std::fs::write(&placeholder_path, placeholder_bytes)
        .map_err(|err| AppError::from(format!("build_artifacts: write placeholder: {}", err)))?;

    Ok(ShardOutput {
        path: placeholder_path,
        byte_count: placeholder_bytes.len() as u64,
    })
}
