//! Hashing is split from renaming in two phases: every file is hashed
//! first, and only if the entire batch succeeded do we rename. If one
//! file fails, no file is left renamed; the next build can be re-run
//! cleanly.

use std::fs;
use std::iter;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::artifact::artifact_model::{HashedArtifacts, Hashed, StatisticShard, FileReference};
use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};
use crate::error::AppError;

const SHA_PREFIX_LEN: usize = 8;

pub fn compute_content_hashes(
    shards: Vec<FileReference>,
    geometry: FileReference,
) -> Result<HashedArtifacts, AppError> {
    let mut hash_plan: Vec<(FileReference, String)> = Vec::with_capacity(shards.len() + 1);

    for shard in shards.iter().chain(iter::once(&geometry)) {
        let sha256_hex: String = sha256_hex_of_file(&shard.path)?;
        hash_plan.push((shard.clone(), sha256_hex));
    }

    let geometry_plan: (FileReference, String) = hash_plan.pop().expect("geometry hash plan");
    let statistic_plan: Vec<(FileReference, String)> = hash_plan;

    let geometry: Hashed<FileReference> = rename_to_content_hashed(geometry_plan.0, &geometry_plan.1)?;

    let statistic_shards: Vec<StatisticShard> = statistic_plan
        .into_iter()
        .map(|(tmp_file, sha256_hex)| {
            let (statistic_kind, license_shard_class) = parse_statistic_shard_filename(&tmp_file.path)?;
            let renamed: Hashed<FileReference> = rename_to_content_hashed(tmp_file, &sha256_hex)?;
            Ok(StatisticShard {
                statistic_kind,
                license_shard_class,
                hashed_file: renamed,
            })
        })
        .collect::<Result<Vec<StatisticShard>, AppError>>()?;

    Ok(HashedArtifacts {
        statistic_shards,
        geometry,
    })
}

fn sha256_hex_of_file(path: &Path) -> Result<String, AppError> {
    let bytes: Vec<u8> = fs::read(path)?;
    let mut hasher: Sha256 = Sha256::new();
    hasher.update(&bytes);
    let digest: [u8; 32] = hasher.finalize().into();
    Ok(hex_encode(&digest))
}

fn rename_to_content_hashed(tmp_file: FileReference, sha256_hex: &str) -> Result<Hashed<FileReference>, AppError> {
    let new_path: PathBuf = build_hashed_path(&tmp_file.path, sha256_hex)?;
    fs::rename(&tmp_file.path, &new_path).map_err(|err| {
        AppError::from(format!(
            "rename {:?} -> {:?}: {}",
            tmp_file.path, new_path, err,
        ))
    })?;
    Ok(Hashed {
        inner: FileReference {
            path: new_path,
            byte_count: tmp_file.byte_count,
        },
        sha256_hex: sha256_hex.to_string(),
    })
}

fn build_hashed_path(tmp_path: &Path, sha256_hex: &str) -> Result<PathBuf, AppError> {
    let parent: &Path = tmp_path.parent().ok_or_else(|| {
        AppError::from(format!("no parent for {:?}", tmp_path))
    })?;
    let filename: &str = tmp_path
        .file_name()
        .and_then(|os| os.to_str())
        .ok_or_else(|| AppError::from(format!("bad filename {:?}", tmp_path)))?;

    let (name_part, extension): (&str, &str) = filename
        .rsplit_once('.')
        .ok_or_else(|| AppError::from(format!("no extension in {:?}", filename)))?;

    let stem_without_uuid: &str = trim_tmp_uuid_segment(name_part).ok_or_else(|| {
        AppError::from(format!(
            "filename {:?} missing -tmp.<uuid> segment",
            filename,
        ))
    })?;

    let sha_prefix: &str = sha256_hex
        .get(..SHA_PREFIX_LEN)
        .ok_or_else(|| AppError::from(format!("short hash {}", sha256_hex)))?;

    Ok(parent.join(format!("{}-{}.{}", stem_without_uuid, sha_prefix, extension)))
}

fn trim_tmp_uuid_segment(name_part: &str) -> Option<&str> {
    let (stem, _uuid_part): (&str, &str) = name_part.rsplit_once("-tmp.")?;
    Some(stem)
}

fn parse_statistic_shard_filename(path: &Path) -> Result<(StatisticKind, LicenseShardClass), AppError> {
    let filename: &str = path
        .file_name()
        .and_then(|os| os.to_str())
        .ok_or_else(|| AppError::from(format!("bad path {:?}", path)))?;

    let stem: &str = filename.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(filename);
    let stem_without_uuid: &str =
        trim_tmp_uuid_segment(stem).ok_or_else(|| {
            AppError::from(format!(
                "missing -tmp. in {}",
                filename,
            ))
        })?;

    let (statistic_code, license_part): (&str, &str) = stem_without_uuid
        .rsplit_once('-')
        .ok_or_else(|| AppError::from(format!("no license suffix in {}", filename)))?;

    let statistic_kind: StatisticKind = StatisticKind::try_from(statistic_code)?;
    let license_shard_class: LicenseShardClass = LicenseShardClass::try_from(license_part)?;

    Ok((statistic_kind, license_shard_class))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut hex_string: String = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex_string.push_str(&format!("{:02x}", byte));
    }
    hex_string
}

#[cfg(test)]
mod tests {
    use super::*;

    use uuid::Uuid;

    fn write_tmp_shard(temp_dir: &Path, filename: &str, contents: &[u8]) -> FileReference {
        let path: PathBuf = temp_dir.join(filename);
        fs::write(&path, contents).unwrap();
        FileReference {
            path,
            byte_count: contents.len() as u64,
        }
    }

    fn make_shard_files(temp_dir: &Path) -> (Vec<FileReference>, FileReference) {
        let tmp_uuid: Uuid = Uuid::now_v7();
        let shard: FileReference = write_tmp_shard(
            temp_dir,
            &format!("tfr-base-tmp.{}.sqlite", tmp_uuid),
            b"SQLITE FAKE",
        );
        let geometry: FileReference = write_tmp_shard(
            temp_dir,
            &format!("world-50m-tmp.{}.fgb", tmp_uuid),
            b"FGB FAKE",
        );
        (vec![shard], geometry)
    }

    #[test]
    fn compute_content_hashes_matches_sha256_over_file_bytes() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, geometry) = make_shard_files(temp_dir.path());

        let hashed: HashedArtifacts = compute_content_hashes(shards, geometry).unwrap();

        let mut hasher: Sha256 = Sha256::new();
        hasher.update(b"SQLITE FAKE");
        let expected: String = hex_encode(&Into::<[u8; 32]>::into(hasher.finalize()));
        assert_eq!(hashed.statistic_shards[0].hashed_file.sha256_hex, expected);
    }

    #[test]
    fn compute_content_hashes_renames_tmp_files_to_sha8_filenames() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, geometry) = make_shard_files(temp_dir.path());
        let original_shard_path: PathBuf = shards[0].path.clone();
        let original_geometry_path: PathBuf = geometry.path.clone();

        let hashed: HashedArtifacts = compute_content_hashes(shards, geometry).unwrap();

        assert!(!original_shard_path.exists());
        assert!(!original_geometry_path.exists());
        assert!(hashed.statistic_shards[0].hashed_file.path.exists());
        assert!(hashed.geometry.path.exists());

        let shard_filename: String = hashed.statistic_shards[0]
            .hashed_file
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(shard_filename.starts_with("tfr-base-"));
        assert!(shard_filename.ends_with(".sqlite"));
        assert!(!shard_filename.contains("-tmp."));

        let geometry_filename: String = hashed
            .geometry
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(geometry_filename.starts_with("world-50m-"));
        assert!(geometry_filename.ends_with(".fgb"));
    }

    #[test]
    fn compute_content_hashes_is_idempotent_in_value_for_same_bytes() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards_one, geometry_one) = make_shard_files(temp_dir.path());

        let temp_dir_two: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards_two, geometry_two) = make_shard_files(temp_dir_two.path());

        let hashed_one: HashedArtifacts = compute_content_hashes(shards_one, geometry_one).unwrap();
        let hashed_two: HashedArtifacts = compute_content_hashes(shards_two, geometry_two).unwrap();

        assert_eq!(
            hashed_one.statistic_shards[0].hashed_file.sha256_hex,
            hashed_two.statistic_shards[0].hashed_file.sha256_hex,
        );
        assert_eq!(
            hashed_one.geometry.sha256_hex,
            hashed_two.geometry.sha256_hex,
        );
    }

    #[test]
    fn compute_content_hashes_aborts_without_renaming_if_one_file_missing() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, geometry) = make_shard_files(temp_dir.path());
        let surviving_path: PathBuf = shards[0].path.clone();

        let mut shards: Vec<FileReference> = shards;
        shards.push(FileReference {
            path: temp_dir.path().join("missing-base-tmp.deadbeef.sqlite"),
            byte_count: 0,
        });

        let result: Result<HashedArtifacts, AppError> = compute_content_hashes(shards, geometry);

        assert!(result.is_err());
        assert!(surviving_path.exists());
    }

    #[test]
    fn parse_statistic_shard_filename_recognizes_all_license_classes() {
        let cases: [(&str, LicenseShardClass); 3] = [
            ("tfr-base-tmp.x.sqlite", LicenseShardClass::Base),
            ("tfr-share_alike-tmp.x.sqlite", LicenseShardClass::ShareAlike),
            ("tfr-noncommercial-tmp.x.sqlite", LicenseShardClass::NonCommercial),
        ];
        for (filename, expected) in cases {
            let path: PathBuf = PathBuf::from(filename);
            let (_, license_shard_class) = parse_statistic_shard_filename(&path).unwrap();
            assert_eq!(license_shard_class, expected);
        }
    }
}
