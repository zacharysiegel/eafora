//! Runs between the writers and the content-addressed rename, so the digest and the byte count in the manifest
//! describe the bytes a client fetches.

use std::fs;
use std::path::{Path, PathBuf};

use shared::artifact::compression;
use shared::filesystem::FileReference;

use crate::artifact::artifact_model::StatisticShard;
use crate::error::AppError;

/// An artifact as a writer leaves it. Compressing consumes it, because the file it names is then gone.
pub struct PlainArtifact {
    pub file: FileReference,
}

/// An artifact in the form it is published and hashed in, which is what the manifest's digest and size
/// describe. Only compression produces one, so nothing can hash or upload a file that skipped the step.
pub struct CompressedArtifact {
    pub file: FileReference,
}

pub fn compress_shards(
    shards: Vec<StatisticShard<PlainArtifact>>,
) -> Result<Vec<StatisticShard<CompressedArtifact>>, AppError> {
    shards
        .into_iter()
        .map(|shard| {
            let compressed: CompressedArtifact = compress_artifact(shard.file)?;

            Ok(StatisticShard {
                key: shard.key,
                file: compressed,
            })
        })
        .collect()
}

pub fn compress_artifact(plain: PlainArtifact) -> Result<CompressedArtifact, AppError> {
    let plain_path: &Path = &plain.file.path;
    let plain_bytes: Vec<u8> = fs::read(plain_path)
        .map_err(|error| AppError::from(format!("reading {plain_path:?} to compress failed; [error={error}]")))?;

    let compressed_bytes: Vec<u8> = compression::compress(&plain_bytes)?;
    let compressed_path: PathBuf = compressed_sibling_of(plain_path);

    fs::write(&compressed_path, &compressed_bytes)
        .map_err(|error| AppError::from(format!("writing {compressed_path:?} failed; [error={error}]")))?;
    fs::remove_file(plain_path)
        .map_err(|error| AppError::from(format!("removing {plain_path:?} failed; [error={error}]")))?;

    log::debug!(
        "compressed artifact; [path={:?} plain={} compressed={}]",
        compressed_path,
        plain_bytes.len(),
        compressed_bytes.len(),
    );

    Ok(CompressedArtifact {
        file: FileReference {
            path: compressed_path,
            byte_count: compressed_bytes.len() as u64,
        },
    })
}

fn compressed_sibling_of(plain_path: &Path) -> PathBuf {
    let mut filename: std::ffi::OsString = plain_path.file_name().unwrap_or_default().to_os_string();
    filename.push(".");
    filename.push(compression::COMPRESSED_FILENAME_EXTENSION);

    plain_path.with_file_name(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_artifact(path: &Path) -> PlainArtifact {
        PlainArtifact {
            file: FileReference { path: path.to_path_buf(), byte_count: 0 },
        }
    }

    fn write_plain(temp_dir: &Path, filename: &str, contents: &[u8]) -> PathBuf {
        let path: PathBuf = temp_dir.join(filename);
        fs::write(&path, contents).unwrap();

        path
    }

    #[test]
    fn compress_artifact_reports_the_bytes_it_wrote() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let plain_path: PathBuf = write_plain(temp_dir.path(), "tfr-base.tmp-0198.sqlite", &b"redundant ".repeat(500));

        let compressed: CompressedArtifact = compress_artifact(plain_artifact(&plain_path)).unwrap();

        assert_eq!(compressed.file.byte_count, fs::metadata(&compressed.file.path).unwrap().len());
        assert_eq!(compressed.file.path.file_name().unwrap(), "tfr-base.tmp-0198.sqlite.br");
        assert!(!plain_path.exists());
    }

    #[test]
    fn compress_artifact_writes_bytes_the_reader_restores() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let plain_bytes: Vec<u8> = b"redundant ".repeat(500);
        let plain_path: PathBuf = write_plain(temp_dir.path(), "world-50m.tmp-0198.fgb", &plain_bytes);

        let compressed: CompressedArtifact = compress_artifact(plain_artifact(&plain_path)).unwrap();

        let restored: Vec<u8> = compression::decompress(&fs::read(&compressed.file.path).unwrap()).unwrap();
        assert_eq!(restored, plain_bytes);
        assert!(compressed.file.byte_count < plain_bytes.len() as u64);
    }

    #[test]
    fn compress_artifact_errors_when_the_plain_file_is_missing() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();

        assert!(compress_artifact(plain_artifact(&temp_dir.path().join("absent.tmp-0198.fgb"))).is_err());
    }
}
