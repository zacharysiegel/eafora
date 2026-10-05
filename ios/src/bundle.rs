use std::sync::{Arc, OnceLock};

use tokio::sync::watch;

use shared::artifact::{load, Bundle, FilesystemArtifactCache};
use shared::error::AppErrorStatic;
use shared::http::{FilesystemFetch, ReqwestHttpFetch};
use shared::license::DistributionContext;

use crate::cache;
use crate::error::FfiError;

const DISTRIBUTION_CONTEXT: DistributionContext = DistributionContext::FirstParty;

static PUBLICATION: OnceLock<watch::Sender<Arc<Bundle>>> = OnceLock::new();

/// Opens the newest readable cached bundle, falling back to the bundle in `embedded_directory`.
#[uniffi::export(async_runtime = "tokio")]
pub async fn open_first_paint_bundle(embedded_directory: String) -> Result<String, FfiError> {
    let cache: FilesystemArtifactCache = cache::create_cache()?;

    let cached: Option<Bundle> = load::open_newest_cached_bundle(&cache, DISTRIBUTION_CONTEXT).await?;

    let bundle: Bundle = match cached {
        Some(cached) => cached,
        None => load::load_embedded_bundle(&cache, &FilesystemFetch, &embedded_directory, DISTRIBUTION_CONTEXT).await?,
    };

    Ok(publish(bundle))
}

/// Fetches the newest published bundle and republishes it through the channel the renderer holds.
#[uniffi::export(async_runtime = "tokio")]
pub async fn load_live_bundle(discovery_url: String, static_repository_base_url: String) -> Result<String, FfiError> {
    let cache: FilesystemArtifactCache = cache::create_cache()?;
    let http_fetch: ReqwestHttpFetch = ReqwestHttpFetch::create()?;

    let bundle: Bundle = load::load_live_bundle(
        &cache,
        &http_fetch,
        &discovery_url,
        &static_repository_base_url,
        DISTRIBUTION_CONTEXT,
    )
    .await?;

    let version_label: String = publish(bundle);

    let evicted: Result<(), AppErrorStatic> = load::evict_stale_versions(&cache).await;
    if let Err(error) = evicted {
        log::warn!("evicting old cached bundle versions failed; [error={error}]");
    }

    Ok(version_label)
}

/// The receiver the renderer reads the current bundle through. Requires a bundle to have been opened.
pub fn subscribe() -> Result<watch::Receiver<Arc<Bundle>>, FfiError> {
    let sender: Option<&watch::Sender<Arc<Bundle>>> = PUBLICATION.get();

    let Some(sender) = sender
    else {
        return Err(FfiError::Failed {
            message: "no bundle has been opened yet".to_string(),
        });
    };

    Ok(sender.subscribe())
}

/// Replaces the published bundle, creating the channel on the first call, and answers with its version.
fn publish(bundle: Bundle) -> String {
    let version_label: String = bundle.manifest.version.clone();

    publish_to(&PUBLICATION, Arc::new(bundle));

    version_label
}

/// `send_replace` rather than `send`, which fails while no receiver exists.
fn publish_to<T: Clone>(cell: &OnceLock<watch::Sender<T>>, value: T) {
    let sender: &watch::Sender<T> = cell.get_or_init(|| watch::channel(value.clone()).0);

    let _previous_value: T = sender.send_replace(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_to_makes_the_first_value_available_to_a_later_subscriber() {
        let cell: OnceLock<watch::Sender<i32>> = OnceLock::new();

        publish_to(&cell, 1);

        let receiver: watch::Receiver<i32> = cell.get().unwrap().subscribe();
        assert_eq!(*receiver.borrow(), 1);
    }

    #[test]
    fn publish_to_replaces_the_value_while_no_receiver_exists() {
        let cell: OnceLock<watch::Sender<i32>> = OnceLock::new();

        publish_to(&cell, 1);
        publish_to(&cell, 2);

        let receiver: watch::Receiver<i32> = cell.get().unwrap().subscribe();
        assert_eq!(*receiver.borrow(), 2);
    }

    #[test]
    fn publish_to_reaches_a_receiver_which_subscribed_earlier() {
        let cell: OnceLock<watch::Sender<i32>> = OnceLock::new();
        publish_to(&cell, 1);
        let receiver: watch::Receiver<i32> = cell.get().unwrap().subscribe();

        publish_to(&cell, 2);

        assert!(receiver.has_changed().unwrap());
        assert_eq!(*receiver.borrow(), 2);
    }
}
