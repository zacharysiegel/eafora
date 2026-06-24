//! Each file is hashed, then renamed from its tmp filename to a
//! content-addressed name (full sha256 hex). A failed run leaves stray
//! tmp or hashed files in the output directory; the next build writes
//! fresh tmp files with new uuids and produces its own correct output,
//! so cleanup is best-effort (wipe `artifact_dir` between builds).

use std::fs;
use std::path::{Path, PathBuf};

use crate::artifact::artifact_model::StatisticShard;
use crate::error::AppError;
use shared::filesystem::{self, FileReference, Hashed};

pub fn hash_sqlite_shards(
    shards: Vec<StatisticShard<FileReference>>,
) -> Result<Vec<StatisticShard<Hashed<FileReference>>>, AppError> {
    shards
        .into_iter()
        .map(|shard| {
            let sha256_hex: String = filesystem::sha256_hex_of_file(&shard.file.path)?;
            let hashed_file: Hashed<FileReference> = rename_with_digest(shard.file, &sha256_hex)?;
            Ok(StatisticShard {
                key: shard.key,
                file: hashed_file,
            })
        })
        .collect()
}

pub fn hash_geometry(geometry: FileReference) -> Result<Hashed<FileReference>, AppError> {
    let sha256_hex: String = filesystem::sha256_hex_of_file(&geometry.path)?;
    rename_with_digest(geometry, &sha256_hex)
}

fn rename_with_digest(tmp_file: FileReference, sha256_hex: &str) -> Result<Hashed<FileReference>, AppError> {
    let new_path: PathBuf = build_hashed_path(&tmp_file.path, sha256_hex)?;
    fs::rename(&tmp_file.path, &new_path).map_err(|err| {
        AppError::from(format!(
            "rename {:?} -> {:?}: {}",
            tmp_file.path, new_path, err,
        ))
    })?;
    Ok(Hashed::new_with_sha(
        FileReference {
            path: new_path,
            byte_count: tmp_file.byte_count,
        },
        sha256_hex.to_string(),
    ))
}

fn build_hashed_path(tmp_path: &Path, sha256_hex: &str) -> Result<PathBuf, AppError> {
    let parent: &Path = tmp_path.parent().ok_or_else(|| {
        AppError::from(format!("no parent for {:?}", tmp_path))
    })?;
    let filename: &str = filesystem::filename_of(tmp_path)?;

    let (name_part, extension): (&str, &str) = filename
        .rsplit_once('.')
        .ok_or_else(|| AppError::from(format!("no extension in {:?}", filename)))?;

    let stem_without_uuid: &str = trim_tmp_uuid_segment(name_part).ok_or_else(|| {
        AppError::from(format!(
            "filename {:?} missing .tmp-<uuid> segment",
            filename,
        ))
    })?;

    Ok(parent.join(format!("{}-{}.{}", stem_without_uuid, sha256_hex, extension)))
}

fn trim_tmp_uuid_segment(name_part: &str) -> Option<&str> {
    let (stem, _uuid_part): (&str, &str) = name_part.rsplit_once(".tmp-")?;
    Some(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    use shared::artifact::bundle::StatisticShardKey;
    use shared::canonical::canonical_model::{LicenseShardClass, StatisticKind};

    fn write_tmp_file(temp_dir: &Path, filename: &str, contents: &[u8]) -> FileReference {
        let path: PathBuf = temp_dir.join(filename);
        fs::write(&path, contents).unwrap();
        FileReference {
            path,
            byte_count: contents.len() as u64,
        }
    }

    fn make_shard_and_geometry(temp_dir: &Path) -> (Vec<StatisticShard<FileReference>>, FileReference) {
        let tmp_uuid: Uuid = Uuid::now_v7();
        let shard_file: FileReference = write_tmp_file(
            temp_dir,
            &format!("tfr-base.tmp-{}.sqlite", tmp_uuid),
            b"SQLITE FAKE",
        );
        let shard: StatisticShard<FileReference> = StatisticShard {
            key: StatisticShardKey {
                statistic_kind: StatisticKind::Tfr,
                license_shard_class: LicenseShardClass::Base,
            },
            file: shard_file,
        };
        let geometry: FileReference = write_tmp_file(
            temp_dir,
            &format!("world-50m.tmp-{}.fgb", tmp_uuid),
            b"FGB FAKE",
        );
        (vec![shard], geometry)
    }

    #[test]
    fn hash_sqlite_shards_matches_sha256_over_file_bytes() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, _geometry) = make_shard_and_geometry(temp_dir.path());

        let shards: Vec<StatisticShard<Hashed<FileReference>>> = hash_sqlite_shards(shards).unwrap();

        let mut hasher: Sha256 = Sha256::new();
        hasher.update(b"SQLITE FAKE");
        let digest: [u8; 32] = hasher.finalize().into();
        let expected: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(shards[0].file.sha256_hex(), expected);
    }

    #[test]
    fn hash_sqlite_shards_renames_tmp_files_to_sha256_filenames() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, _geometry) = make_shard_and_geometry(temp_dir.path());
        let original_shard_path: PathBuf = shards[0].file.path.clone();

        let shards: Vec<StatisticShard<Hashed<FileReference>>> = hash_sqlite_shards(shards).unwrap();

        assert!(!original_shard_path.exists());
        assert!(shards[0].file.path.exists());

        let shard_filename: String = shards[0]
            .file
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(shard_filename.starts_with("tfr-base-"));
        assert!(shard_filename.ends_with(".sqlite"));
        assert!(!shard_filename.contains(".tmp-"));
    }

    #[test]
    fn hash_geometry_renames_tmp_file_to_sha256_filename() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (_shards, geometry) = make_shard_and_geometry(temp_dir.path());
        let original_geometry_path: PathBuf = geometry.path.clone();

        let geometry: Hashed<FileReference> = hash_geometry(geometry).unwrap();

        assert!(!original_geometry_path.exists());
        assert!(geometry.path.exists());

        let geometry_filename: String = geometry
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(geometry_filename.starts_with("world-50m-"));
        assert!(geometry_filename.ends_with(".fgb"));
    }

    #[test]
    fn hash_sqlite_shards_is_idempotent_in_value_for_same_bytes() {
        let temp_dir_one: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards_one, _geometry_one) = make_shard_and_geometry(temp_dir_one.path());

        let temp_dir_two: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards_two, _geometry_two) = make_shard_and_geometry(temp_dir_two.path());

        let shards_one: Vec<StatisticShard<Hashed<FileReference>>> = hash_sqlite_shards(shards_one).unwrap();
        let shards_two: Vec<StatisticShard<Hashed<FileReference>>> = hash_sqlite_shards(shards_two).unwrap();

        assert_eq!(
            shards_one[0].file.sha256_hex(),
            shards_two[0].file.sha256_hex(),
        );
    }

    #[test]
    fn hash_sqlite_shards_errors_when_file_missing() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let shards: Vec<StatisticShard<FileReference>> = vec![StatisticShard {
            key: StatisticShardKey {
                statistic_kind: StatisticKind::Tfr,
                license_shard_class: LicenseShardClass::Base,
            },
            file: FileReference {
                path: temp_dir.path().join("tfr-base.tmp-deadbeef.sqlite"),
                byte_count: 0,
            },
        }];

        let result = hash_sqlite_shards(shards);

        assert!(result.is_err());
    }
}
