use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::artifact::{artifact_db, content_hashing, source_priority};
use crate::artifact::artifact_model::{
    CandidateValue, HashedOutputs, HashedShard, LocalArtifactBuild, MergedValue, ShardOutput,
};
use crate::artifact::writer::{flatgeobuf, manifest, sqlite};
use crate::artifact::writer::manifest::ManifestEmission;
use crate::canonical::canonical_model::DataSourceKind;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    pub offline: bool,
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

    let data_source_versions: BTreeMap<DataSourceKind, String> = source_priority::collect_data_source_versions(&candidate_values);
    let merged_values: Vec<MergedValue> = source_priority::apply_source_priority(candidate_values);
    log::info!("build_artifacts: merged into {} values", merged_values.len());

    let sqlite_shards: Vec<ShardOutput> = sqlite::emit_sqlite_shards(&merged_values, output_dir)?;
    log::info!("build_artifacts: emitted {} sqlite shards", sqlite_shards.len());

    let geometry_shard: ShardOutput = if options.offline {
        write_placeholder_geometry(output_dir)?
    } else {
        flatgeobuf::emit_geometry_flatgeobuf(&mut *connection, output_dir).await?
    };
    log::info!("build_artifacts: emitted geometry shard {:?}", geometry_shard.path);

    let hashed: HashedOutputs = content_hashing::compute_content_hashes(sqlite_shards, geometry_shard)?;

    let manifest_emission: ManifestEmission =
        manifest::emit_manifest(&hashed, version_label, &data_source_versions, output_dir)?;
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
    executor: impl PgExecutor<'e>,
    candidates: &[CandidateValue],
) -> Result<(), AppError> {
    let known_codes: Vec<String> = artifact_db::read_all_statistic_codes(executor).await?;
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
    fs::create_dir_all(&geometry_dir)?;

    let placeholder_path: PathBuf = geometry_dir.join(format!("world-50m-tmp.{}.fgb", Uuid::now_v7()));
    let placeholder_bytes: &[u8] = b"FGB-PLACEHOLDER";
    fs::write(&placeholder_path, placeholder_bytes)?;

    Ok(ShardOutput {
        path: placeholder_path,
        byte_count: placeholder_bytes.len() as u64,
    })
}
