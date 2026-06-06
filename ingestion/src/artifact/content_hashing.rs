//! Each file is hashed, then renamed from its tmp filename to a
//! content-addressed name (sha256 prefix). A failed run leaves stray
//! tmp or hashed files in the output directory; the next build writes
//! fresh tmp files with new uuids and produces its own correct output,
//! so cleanup is best-effort (wipe `output_dir` between builds).

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::artifact::artifact_model::{FileReference, StatisticShard};
use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};
use crate::error::AppError;

const SHA_PREFIX_LEN: usize = 8;

#[derive(Debug, Clone)]
pub struct Hashed<T> {
    inner: T,
    sha256_hex: String,
}

impl<T> Hashed<T> {
    pub fn new(inner: T, bytes: impl AsRef<[u8]>) -> Self {
        Hashed {
            inner,
            sha256_hex: sha256_hex(bytes.as_ref()),
        }
    }

    pub fn sha256_hex(&self) -> &str {
        &self.sha256_hex
    }

    #[cfg(test)]
    pub fn new_with_sha(inner: T, sha256_hex: String) -> Self {
        Hashed { inner, sha256_hex }
    }
}

impl<T> Deref for Hashed<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.inner }
}

pub fn hash_sqlite_shards(
    shards: Vec<FileReference>,
) -> Result<Vec<StatisticShard>, AppError> {
    shards
        .into_iter()
        .map(|tmp_file| {
            let (statistic_kind, license_shard_class) = parse_statistic_shard_filename(&tmp_file.path)?;
            let sha256_hex: String = sha256_hex_of_file(&tmp_file.path)?;
            let renamed: Hashed<FileReference> = rename_to_content_hashed(tmp_file, &sha256_hex)?;
            Ok(StatisticShard {
                statistic_kind,
                license_shard_class,
                hashed_file: renamed,
            })
        })
        .collect()
}

pub fn hash_geometry(geometry: FileReference) -> Result<Hashed<FileReference>, AppError> {
    let sha256_hex: String = sha256_hex_of_file(&geometry.path)?;
    rename_to_content_hashed(geometry, &sha256_hex)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher: Sha256 = Sha256::new();
    hasher.update(bytes);
    let digest: [u8; 32] = hasher.finalize().into();
    hex_encode(&digest)
}

fn sha256_hex_of_file(path: &Path) -> Result<String, AppError> {
    let bytes: Vec<u8> = fs::read(path)?;
    Ok(sha256_hex(&bytes))
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
    fn hash_shards_matches_sha256_over_file_bytes() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, _geometry) = make_shard_files(temp_dir.path());

        let shards: Vec<StatisticShard> = hash_sqlite_shards(shards).unwrap();

        let mut hasher: Sha256 = Sha256::new();
        hasher.update(b"SQLITE FAKE");
        let expected: String = hex_encode(&Into::<[u8; 32]>::into(hasher.finalize()));
        assert_eq!(shards[0].hashed_file.sha256_hex(), expected);
    }

    #[test]
    fn hash_shards_renames_tmp_files_to_sha8_filenames() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, _geometry) = make_shard_files(temp_dir.path());
        let original_shard_path: PathBuf = shards[0].path.clone();

        let shards: Vec<StatisticShard> = hash_sqlite_shards(shards).unwrap();

        assert!(!original_shard_path.exists());
        assert!(shards[0].hashed_file.path.exists());

        let shard_filename: String = shards[0]
            .hashed_file
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(shard_filename.starts_with("tfr-base-"));
        assert!(shard_filename.ends_with(".sqlite"));
        assert!(!shard_filename.contains("-tmp."));
    }

    #[test]
    fn hash_geometry_renames_tmp_file_to_sha8_filename() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (_shards, geometry) = make_shard_files(temp_dir.path());
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
    fn hash_shards_is_idempotent_in_value_for_same_bytes() {
        let temp_dir_one: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards_one, _geometry_one) = make_shard_files(temp_dir_one.path());

        let temp_dir_two: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards_two, _geometry_two) = make_shard_files(temp_dir_two.path());

        let shards_one: Vec<StatisticShard> = hash_sqlite_shards(shards_one).unwrap();
        let shards_two: Vec<StatisticShard> = hash_sqlite_shards(shards_two).unwrap();

        assert_eq!(
            shards_one[0].hashed_file.sha256_hex(),
            shards_two[0].hashed_file.sha256_hex(),
        );
    }

    #[test]
    fn hash_shards_errors_when_file_missing() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let shards: Vec<FileReference> = vec![FileReference {
            path: temp_dir.path().join("missing-base-tmp.deadbeef.sqlite"),
            byte_count: 0,
        }];

        let result = hash_sqlite_shards(shards);

        assert!(result.is_err());
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
