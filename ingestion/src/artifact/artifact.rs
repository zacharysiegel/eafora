use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use sqlx::PgConnection;

use crate::artifact::artifact_model::{
    Artifacts, BuildReport, CandidateValue, ResolvedValue,
};
use crate::artifact::writer::{flatgeobuf, manifest, sqlite};
use crate::artifact::{artifact_db, hashing, source_choice, StatisticShard};
use crate::canonical::canonical_db;
use crate::canonical::canonical_model::{DataSourceKind, SourceChoice, SourceRevision, StatisticKind};
use crate::error::AppError;
use crate::filesystem::{FileReference, Hashed};

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    pub test_offline: bool,
}

pub async fn build_artifacts(
    connection: &mut PgConnection,
    artifact_dir: &Path,
    version_label: &str,
    options: BuildOptions,
) -> Result<BuildReport, AppError> {
    let started: Instant = Instant::now();
    log::info!("starting version_label={} artifact_dir={:?}", version_label, artifact_dir,);

    fs::create_dir_all(artifact_dir)?;

    let source_choices: Vec<SourceChoice> = canonical_db::read_source_choices(&mut *connection).await?;
    let statistic_kinds: BTreeSet<StatisticKind> = artifact_db::read_all_statistic_kinds(&mut *connection).await?;

    let (shards, data_sources): (Vec<StatisticShard<Hashed<FileReference>>>, BTreeSet<DataSourceKind>) =
        create_statistic_shards(connection, artifact_dir, &source_choices, statistic_kinds).await?;
    let geometry: Hashed<FileReference> = create_geometry(connection, artifact_dir, options).await?;

    let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> =
        artifact_db::read_latest_revisions(&mut *connection, &data_sources).await?;
    let manifest: Hashed<FileReference> =
        manifest::write_manifest(&shards, &geometry, version_label, &data_source_revisions, artifact_dir)?;

    log::info!(
        "complete in {:?}; manifest sha256={}",
        started.elapsed(),
        manifest.sha256_hex(),
    );

    Ok(BuildReport {
        artifact_dir: artifact_dir.to_path_buf(),
        version_label: version_label.to_string(),
        artifacts: Artifacts {
            shards,
            geometry,
            manifest,
        },
        data_source_revisions,
    })
}

async fn create_statistic_shards(
    connection: &mut PgConnection,
    artifact_dir: &Path,
    source_choices: &Vec<SourceChoice>,
    statistic_kinds: BTreeSet<StatisticKind>,
) -> Result<(Vec<StatisticShard<Hashed<FileReference>>>, BTreeSet<DataSourceKind>), AppError> {
    let mut shards: Vec<StatisticShard<Hashed<FileReference>>> = Vec::new();
    let mut data_sources: BTreeSet<DataSourceKind> = BTreeSet::new();

    for kind in statistic_kinds {
        let candidates: Vec<CandidateValue> =
            artifact_db::read_candidate_values_for_statistic(&mut *connection, kind).await?;
        if candidates.is_empty() {
            log::warn!(
                "statistic {:?} has no candidate values; shard will be missing from this build",
                kind
            );
            continue;
        }

        for candidate in &candidates {
            data_sources.insert(candidate.data_source_kind);
        }

        let resolved: Vec<ResolvedValue> = source_choice::resolve_candidates(candidates, &source_choices)?;
        let tmp_shards: Vec<StatisticShard<FileReference>> = sqlite::write_sqlite_shards(&resolved, &artifact_dir.join(manifest::SUBDIR_DATA))?;
        let hashed_shards: Vec<StatisticShard<Hashed<FileReference>>> = hashing::hash_sqlite_shards(tmp_shards)?;
        log::info!(
            "statistic {:?}: {} resolved values across {} shards",
            kind,
            resolved.len(),
            hashed_shards.len()
        );
        shards.extend(hashed_shards);
    }

    Ok((shards, data_sources))
}

async fn create_geometry(
    connection: &mut PgConnection,
    artifact_dir: &Path,
    options: BuildOptions,
) -> Result<Hashed<FileReference>, AppError> {
    let geometry: FileReference = if options.test_offline {
        flatgeobuf::write_placeholder_geometry(artifact_dir)?
    } else {
        flatgeobuf::write_geometry(&mut *connection, artifact_dir).await?
    };
    log::info!("wrote geometry {:?}", geometry.path);
    let geometry: Hashed<FileReference> = hashing::hash_geometry(geometry)?;
    Ok(geometry)
}
