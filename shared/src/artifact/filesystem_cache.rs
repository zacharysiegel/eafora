use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::artifact::ArtifactCache;
use crate::error::AppErrorStatic;

/// An [`ArtifactCache`] over a directory tree, holding each version's files under `<root>/<version_label>/`.
/// A root that has been removed underneath a running process reads as an empty cache, which is what an
/// operating system reclaiming a cache directory looks like from inside.
pub struct FilesystemArtifactCache {
    root: PathBuf,
}

impl FilesystemArtifactCache {
    pub fn create(root: PathBuf) -> FilesystemArtifactCache {
        FilesystemArtifactCache { root }
    }

    fn file_path(&self, version_label: &str, file_relative_path: &str) -> Result<PathBuf, AppErrorStatic> {
        let mut path: PathBuf = self.root.join(validated_segment(version_label)?);

        for segment in file_relative_path.split('/').filter(|segment| !segment.is_empty()) {
            path.push(validated_segment(segment)?);
        }

        Ok(path)
    }
}

impl ArtifactCache for FilesystemArtifactCache {
    async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppErrorStatic> {
        let path: PathBuf = self.file_path(version_label, file_relative_path)?;

        let parent: &Path = path
            .parent()
            .ok_or_else(|| AppErrorStatic::from(format!("cache path has no parent directory; [path={}]", path.display())))?;

        fs::create_dir_all(parent).map_err(|error| {
            AppErrorStatic::from(format!("creating a cache directory failed; [path={} error={error}]", parent.display()))
        })?;

        fs::write(&path, bytes).map_err(|error| {
            AppErrorStatic::from(format!("writing a cached file failed; [path={} error={error}]", path.display()))
        })?;

        Ok(())
    }

    async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppErrorStatic> {
        let path: PathBuf = self.file_path(version_label, file_relative_path)?;

        let read: Result<Vec<u8>, std::io::Error> = fs::read(&path);

        match read {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AppErrorStatic::from(format!(
                "reading a cached file failed; [path={} error={error}]",
                path.display(),
            ))),
        }
    }

    async fn list_versions(&self) -> Result<Vec<String>, AppErrorStatic> {
        let entries: Result<fs::ReadDir, std::io::Error> = fs::read_dir(&self.root);

        let entries: fs::ReadDir = match entries {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(AppErrorStatic::from(format!(
                    "listing cached versions failed; [path={} error={error}]",
                    self.root.display(),
                )))
            }
        };

        let mut version_labels: Vec<String> = Vec::new();

        for entry in entries {
            let entry: fs::DirEntry = entry.map_err(|error| {
                AppErrorStatic::from(format!(
                    "reading a cache directory entry failed; [path={} error={error}]",
                    self.root.display(),
                ))
            })?;

            let is_directory: bool = entry.file_type().map(|file_type| file_type.is_dir()).unwrap_or(false);
            if !is_directory {
                continue;
            }

            let Some(version_label) = entry.file_name().to_str().map(str::to_string)
            else {
                continue;
            };

            version_labels.push(version_label);
        }

        version_labels.sort();

        Ok(version_labels)
    }

    async fn delete_version(&self, version_label: &str) -> Result<(), AppErrorStatic> {
        let path: PathBuf = self.root.join(validated_segment(version_label)?);

        let removed: Result<(), std::io::Error> = fs::remove_dir_all(&path);

        match removed {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppErrorStatic::from(format!(
                "deleting a cached version failed; [path={} error={error}]",
                path.display(),
            ))),
        }
    }
}

/// Every segment must name a child. An empty or `.` segment resolves to the cache root and `..` to above
/// it, so any of them would have `delete_version` remove a directory the caller did not name.
fn validated_segment(segment: &str) -> Result<&str, AppErrorStatic> {
    if segment.is_empty() || segment == "." || segment == ".." {
        return Err(AppErrorStatic::from(format!("cache path segment does not name a child; [segment={segment:?}]")));
    }

    Ok(segment)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn create_cache() -> (TempDir, FilesystemArtifactCache) {
        let root: TempDir = TempDir::new().unwrap();
        let cache: FilesystemArtifactCache = FilesystemArtifactCache::create(root.path().join("artifacts"));

        (root, cache)
    }

    #[tokio::test]
    async fn put_then_get_returns_the_written_bytes() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();

        cache.put("2026-08-14+macdiarmid", "data/tfr.sqlite", b"shard").await.unwrap();

        let bytes: Option<Vec<u8>> = cache.get("2026-08-14+macdiarmid", "data/tfr.sqlite").await.unwrap();
        assert_eq!(bytes.as_deref(), Some(b"shard".as_slice()));
    }

    #[tokio::test]
    async fn get_returns_none_for_a_file_that_was_never_written() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();

        let bytes: Option<Vec<u8>> = cache.get("2026-08-14+macdiarmid", "data/absent.sqlite").await.unwrap();

        assert_eq!(bytes, None);
    }

    #[tokio::test]
    async fn list_versions_returns_only_directories_sorted() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();
        cache.put("2026-08-14+macdiarmid", "manifest.json", b"{}").await.unwrap();
        cache.put("2026-07-01+yeats", "manifest.json", b"{}").await.unwrap();
        fs::write(cache.root.join(".DS_Store"), b"junk").unwrap();

        let version_labels: Vec<String> = cache.list_versions().await.unwrap();

        assert_eq!(version_labels, vec!["2026-07-01+yeats".to_string(), "2026-08-14+macdiarmid".to_string()]);
    }

    #[tokio::test]
    async fn delete_version_removes_only_that_version() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();
        cache.put("2026-08-14+macdiarmid", "manifest.json", b"a").await.unwrap();
        cache.put("2026-07-01+yeats", "manifest.json", b"b").await.unwrap();

        cache.delete_version("2026-08-14+macdiarmid").await.unwrap();

        assert_eq!(cache.get("2026-08-14+macdiarmid", "manifest.json").await.unwrap(), None);
        assert_eq!(cache.get("2026-07-01+yeats", "manifest.json").await.unwrap().as_deref(), Some(b"b".as_slice()));
    }

    #[tokio::test]
    async fn delete_version_accepts_a_version_that_is_not_cached() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();

        cache.delete_version("2026-08-14+macdiarmid").await.unwrap();
    }

    /// The operating system may reclaim a cache directory while the process holding this cache runs. The
    /// loader has to see an empty cache and refetch, not an error it cannot act on.
    #[tokio::test]
    async fn a_root_removed_mid_session_reads_as_an_empty_cache_and_accepts_new_writes() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();
        cache.put("2026-08-14+macdiarmid", "data/tfr.sqlite", b"shard").await.unwrap();

        fs::remove_dir_all(&cache.root).unwrap();

        assert_eq!(cache.get("2026-08-14+macdiarmid", "data/tfr.sqlite").await.unwrap(), None);
        assert_eq!(cache.list_versions().await.unwrap(), Vec::<String>::new());
        cache.delete_version("2026-08-14+macdiarmid").await.unwrap();

        cache.put("2026-08-14+macdiarmid", "data/tfr.sqlite", b"refetched").await.unwrap();
        assert_eq!(
            cache.get("2026-08-14+macdiarmid", "data/tfr.sqlite").await.unwrap().as_deref(),
            Some(b"refetched".as_slice()),
        );
    }

    #[tokio::test]
    async fn put_rejects_a_relative_path_reaching_outside_the_cache_root() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();

        let error: AppErrorStatic = cache
            .put("2026-08-14+macdiarmid", "../escaped.sqlite", b"shard")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("does not name a child"));
    }

    #[tokio::test]
    async fn put_rejects_a_version_label_reaching_outside_the_cache_root() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();

        let error: AppErrorStatic = cache.put("..", "manifest.json", b"{}").await.unwrap_err();

        assert!(error.to_string().contains("does not name a child"));
    }

    /// `.` resolves to the cache root, so accepting it would delete every version rather than one.
    #[tokio::test]
    async fn delete_version_rejects_a_label_naming_the_cache_root() {
        let (_root, cache): (TempDir, FilesystemArtifactCache) = create_cache();
        cache.put("2026-08-14+macdiarmid", "manifest.json", b"{}").await.unwrap();

        let error: AppErrorStatic = cache.delete_version(".").await.unwrap_err();

        assert!(error.to_string().contains("does not name a child"));
        assert!(cache.get("2026-08-14+macdiarmid", "manifest.json").await.unwrap().is_some());
    }
}
