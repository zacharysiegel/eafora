use crate::error::AppError;

// Stable async-fn-in-trait (no async-trait crate). The trait deliberately omits a Send bound on
// the returned futures: the web cache impl (OpfsArtifactCache) holds !Send JsValue handles, so a
// Send bound would make the web impl impossible, and one trait must serve every platform. Native
// targets ARE multi-threaded, but that's fine — only the resulting Arc<Bundle> (Send + Sync) crosses
// threads, via the watch channel; the cache future is awaited within the loader, not sent across threads.
#[allow(async_fn_in_trait)]
pub trait ArtifactCache {
    async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppError>;
    async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError>;
    async fn list_versions(&self) -> Result<Vec<String>, AppError>;
    async fn delete_version(&self, version_label: &str) -> Result<(), AppError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use super::*;

    pub(crate) struct MockArtifactCache {
        entries: tokio::sync::Mutex<BTreeMap<(String, String), Vec<u8>>>,
    }

    impl MockArtifactCache {
        pub(crate) fn new() -> Self {
            MockArtifactCache {
                entries: tokio::sync::Mutex::new(BTreeMap::new()),
            }
        }

        pub(crate) async fn insert(&self, version_label: &str, file_relative_path: &str, bytes: Vec<u8>) {
            let mut entries = self.entries.lock().await;
            entries.insert((version_label.to_string(), file_relative_path.to_string()), bytes);
        }
    }

    impl ArtifactCache for MockArtifactCache {
        async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppError> {
            self.insert(version_label, file_relative_path, bytes.to_vec()).await;
            Ok(())
        }

        async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError> {
            let entries = self.entries.lock().await;
            let bytes: Option<Vec<u8>> = entries
                .get(&(version_label.to_string(), file_relative_path.to_string()))
                .cloned();

            Ok(bytes)
        }

        async fn list_versions(&self) -> Result<Vec<String>, AppError> {
            let entries = self.entries.lock().await;
            let mut version_labels: Vec<String> = entries.keys().map(|(version_label, _)| version_label.clone()).collect();
            version_labels.dedup();

            Ok(version_labels)
        }

        async fn delete_version(&self, version_label: &str) -> Result<(), AppError> {
            let mut entries = self.entries.lock().await;
            entries.retain(|(entry_version_label, _), _| entry_version_label != version_label);

            Ok(())
        }
    }

    #[tokio::test]
    async fn mock_cache_put_get_round_trip_returns_byte_equal() {
        let cache: MockArtifactCache = MockArtifactCache::new();
        cache.put("v1", "manifest.json", b"hello").await.unwrap();

        let bytes: Option<Vec<u8>> = cache.get("v1", "manifest.json").await.unwrap();

        assert_eq!(bytes.as_deref(), Some(b"hello".as_slice()));
    }

    #[tokio::test]
    async fn mock_cache_get_missing_returns_none() {
        let cache: MockArtifactCache = MockArtifactCache::new();

        let bytes: Option<Vec<u8>> = cache.get("v1", "absent.json").await.unwrap();

        assert_eq!(bytes, None);
    }

    #[tokio::test]
    async fn mock_cache_list_versions_returns_inserted_keys() {
        let cache: MockArtifactCache = MockArtifactCache::new();
        cache.insert("v1", "manifest.json", b"a".to_vec()).await;
        cache.insert("v1", "data/x.sqlite", b"b".to_vec()).await;
        cache.insert("v2", "manifest.json", b"c".to_vec()).await;

        let version_labels: Vec<String> = cache.list_versions().await.unwrap();

        assert_eq!(version_labels, vec!["v1".to_string(), "v2".to_string()]);
    }

    #[tokio::test]
    async fn mock_cache_delete_version_removes_only_that_version() {
        let cache: MockArtifactCache = MockArtifactCache::new();
        cache.insert("v1", "manifest.json", b"a".to_vec()).await;
        cache.insert("v2", "manifest.json", b"b".to_vec()).await;

        cache.delete_version("v1").await.unwrap();

        assert_eq!(cache.get("v1", "manifest.json").await.unwrap(), None);
        assert_eq!(cache.get("v2", "manifest.json").await.unwrap().as_deref(), Some(b"b".as_slice()));
    }
}
