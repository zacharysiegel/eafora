//! The published file is the compressed file, so this step runs between the writers and the content-addressed
//! rename: the digest and the byte count that reach the manifest then describe the bytes a client fetches.

use std::fs;
use std::path::{Path, PathBuf};

use shared::artifact::compression;
use shared::filesystem::FileReference;

use crate::artifact::artifact_model::StatisticShard;
use crate::error::AppError;

pub fn compress_shards(
    shards: Vec<StatisticShard<FileReference>>,
) -> Result<Vec<StatisticShard<FileReference>>, AppError> {
    shards
        .into_iter()
        .map(|shard| {
            let compressed_file: FileReference = compress_artifact(&shard.file.path)?;

            Ok(StatisticShard {
                key: shard.key,
                file: compressed_file,
            })
        })
        .collect()
}

/// Replaces the plain temporary artifact with a `.br` sibling and reports the bytes written, so nothing
/// downstream can read a size or a digest from a file that is no longer the one published.
pub fn compress_artifact(plain_path: &Path) -> Result<FileReference, AppError> {
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

    Ok(FileReference {
        path: compressed_path,
        byte_count: compressed_bytes.len() as u64,
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

    fn write_plain(temp_dir: &Path, filename: &str, contents: &[u8]) -> PathBuf {
        let path: PathBuf = temp_dir.join(filename);
        fs::write(&path, contents).unwrap();

        path
    }

    #[test]
    fn compress_artifact_reports_the_bytes_it_wrote() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let plain_path: PathBuf = write_plain(temp_dir.path(), "tfr-base.tmp-0198.sqlite", &b"redundant ".repeat(500));

        let compressed: FileReference = compress_artifact(&plain_path).unwrap();

        assert_eq!(compressed.byte_count, fs::metadata(&compressed.path).unwrap().len());
        assert_eq!(compressed.path.file_name().unwrap(), "tfr-base.tmp-0198.sqlite.br");
        assert!(!plain_path.exists());
    }

    #[test]
    fn compress_artifact_writes_bytes_the_reader_restores() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let plain_bytes: Vec<u8> = b"redundant ".repeat(500);
        let plain_path: PathBuf = write_plain(temp_dir.path(), "world-50m.tmp-0198.fgb", &plain_bytes);

        let compressed: FileReference = compress_artifact(&plain_path).unwrap();

        let restored: Vec<u8> = compression::decompress(&fs::read(&compressed.path).unwrap()).unwrap();
        assert_eq!(restored, plain_bytes);
        assert!(compressed.byte_count < plain_bytes.len() as u64);
    }

    #[test]
    fn compress_artifact_errors_when_the_plain_file_is_missing() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();

        assert!(compress_artifact(&temp_dir.path().join("absent.tmp-0198.fgb")).is_err());
    }
}
