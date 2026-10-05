use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::watch;

use shared::artifact::{load, Bundle, FilesystemArtifactCache};
use shared::error::AppErrorStatic;
use shared::http::{FilesystemFetch, ReqwestHttpFetch};
use shared::license::DistributionContext;

use crate::cache;
use crate::error::FfiError;

const DISTRIBUTION_CONTEXT: DistributionContext = DistributionContext::FirstParty;

static PUBLICATION: Mutex<Option<BundlePublication>> = Mutex::new(None);

/// The published bundle, once one has been opened; the renderer subscribes to the receiver.
struct BundlePublication {
    sender: watch::Sender<Arc<Bundle>>,
    receiver: watch::Receiver<Arc<Bundle>>,
}

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
    let publication: MutexGuard<'_, Option<BundlePublication>> =
        PUBLICATION.lock().expect("the publication mutex is never poisoned");

    let Some(publication) = publication.as_ref()
    else {
        return Err(FfiError::Failed {
            message: "no bundle has been opened yet".to_string(),
        });
    };

    Ok(publication.receiver.clone())
}

/// Replaces the published bundle, creating the channel on the first call, and answers with its version.
fn publish(bundle: Bundle) -> String {
    let version_label: String = bundle.manifest.version.clone();
    let bundle: Arc<Bundle> = Arc::new(bundle);
    let mut publication: MutexGuard<'_, Option<BundlePublication>> =
        PUBLICATION.lock().expect("the publication mutex is never poisoned");

    match publication.as_ref() {
        Some(existing) => {
            let sent: Result<(), watch::error::SendError<Arc<Bundle>>> = existing.sender.send(bundle);

            if let Err(error) = sent {
                log::warn!("publishing a bundle failed; [error={error}]");
            }
        }
        None => {
            let (sender, receiver): (watch::Sender<Arc<Bundle>>, watch::Receiver<Arc<Bundle>>) =
                watch::channel(bundle);

            *publication = Some(BundlePublication { sender, receiver });
        }
    }

    version_label
}
