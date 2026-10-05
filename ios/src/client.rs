use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use tokio::sync::watch;

use shared::artifact::{load, Bundle, FilesystemArtifactCache};
use shared::http::ReqwestHttpFetch;
use shared::license::DistributionContext;
use shared::map::{Renderer, RendererBackend};
use shared::error::{AppError, AppErrorStatic};

use crate::distribution::FfiDistributionContext;
use crate::error::FfiError;
use crate::handle::UiKitSurfaceHandle;

static CACHE_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

/* wgpu state is bound to its creating thread, and UniFFI requires every exported object to be Send + Sync.
   Only the synchronous functions below touch this; an async export could resume on another thread. */
thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

/// The published bundle, once one has been opened; the renderer subscribes to the receiver.
struct BundlePublication {
    sender: watch::Sender<Arc<Bundle>>,
    receiver: watch::Receiver<Arc<Bundle>>,
}

#[derive(uniffi::Object)]
pub struct EaforaClient {
    http_fetch: ReqwestHttpFetch,
    distribution_context: DistributionContext,
    publication: Mutex<Option<BundlePublication>>,
    /// Drives the renderer's asynchronous setup on the calling thread.
    renderer_setup_runtime: tokio::runtime::Runtime,
}

#[uniffi::export(async_runtime = "tokio")]
impl EaforaClient {
    #[uniffi::constructor]
    pub fn new(distribution_context: FfiDistributionContext) -> Result<EaforaClient, FfiError> {
        let http_fetch: ReqwestHttpFetch = ReqwestHttpFetch::create()?;
        let renderer_setup_runtime: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|error| FfiError::Failed {
                message: format!("building the renderer setup runtime failed; [error={error}]"),
            })?;

        Ok(EaforaClient {
            http_fetch,
            distribution_context: DistributionContext::from(distribution_context),
            publication: Mutex::new(None),
            renderer_setup_runtime,
        })
    }

    /// Opens the newest readable cached bundle, falling back to the files shipped in the app bundle.
    pub async fn open_first_paint_bundle(&self, embedded_base_url: String) -> Result<String, FfiError> {
        let cache: FilesystemArtifactCache = create_cache()?;

        let cached: Option<Bundle> = load::open_newest_cached_bundle(&cache, self.distribution_context).await?;

        let bundle: Bundle = match cached {
            Some(cached) => cached,
            None => {
                load::load_embedded_bundle(
                    &cache,
                    &self.http_fetch,
                    &embedded_base_url,
                    self.distribution_context,
                )
                .await?
            }
        };

        Ok(self.publish(bundle))
    }

    /// Fetches the newest published bundle and republishes it through the channel the renderer holds.
    pub async fn load_live_bundle(
        &self,
        discovery_url: String,
        static_repository_base_url: String,
    ) -> Result<String, FfiError> {
        let cache: FilesystemArtifactCache = create_cache()?;

        let bundle: Bundle = load::load_live_bundle(
            &cache,
            &self.http_fetch,
            &discovery_url,
            &static_repository_base_url,
            self.distribution_context,
        )
        .await?;

        let version_label: String = self.publish(bundle);

        let evicted: Result<(), AppErrorStatic> = load::evict_stale_versions(&cache).await;
        if let Err(error) = evicted {
            log::warn!("evicting old cached bundle versions failed; [error={error}]");
        }

        Ok(version_label)
    }

    /// Builds the renderer on the calling thread, which every later renderer function must also use.
    /// Requires a published bundle; the renderer reads its geometry at construction.
    pub fn create_renderer(&self) -> Result<(), FfiError> {
        let receiver: watch::Receiver<Arc<Bundle>> = self.subscribe()?;

        let created: Result<Renderer, AppError> = self
            .renderer_setup_runtime
            .block_on(Renderer::new(receiver, RendererBackend::Default));
        let renderer: Renderer = created?;

        RENDERER.with_borrow_mut(|slot| slot.replace(renderer));

        Ok(())
    }

    pub fn attach_surface(&self, handle: UiKitSurfaceHandle, width: u32, height: u32) -> Result<(), FfiError> {
        self.with_renderer(|renderer| {
            self.renderer_setup_runtime.block_on(renderer.attach_surface_from_window_handle(
                handle.to_window_handle(),
                width,
                height,
            ))
        })
    }

    pub fn resize_surface(&self, width: u32, height: u32) -> Result<(), FfiError> {
        self.with_renderer(|renderer| renderer.resize_surface(width, height))
    }

    pub fn detach_surface(&self) -> Result<(), FfiError> {
        self.with_renderer(|renderer| {
            renderer.detach_surface();

            Ok(())
        })
    }

    pub fn destroy_renderer(&self) {
        RENDERER.with_borrow_mut(|slot| slot.take());
    }
}

impl EaforaClient {
    /// Replaces the published bundle, creating the channel on the first call, and answers with its version.
    fn publish(&self, bundle: Bundle) -> String {
        let version_label: String = bundle.manifest.version.clone();
        let bundle: Arc<Bundle> = Arc::new(bundle);
        let mut publication: MutexGuard<'_, Option<BundlePublication>> = self
            .publication
            .lock()
            .expect("the publication mutex is never poisoned");

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

    fn subscribe(&self) -> Result<watch::Receiver<Arc<Bundle>>, FfiError> {
        let publication: MutexGuard<'_, Option<BundlePublication>> = self
            .publication
            .lock()
            .expect("the publication mutex is never poisoned");

        let Some(publication) = publication.as_ref()
        else {
            return Err(FfiError::Failed {
                message: "no bundle has been opened yet".to_string(),
            });
        };

        Ok(publication.receiver.clone())
    }

    fn with_renderer(
        &self,
        body: impl FnOnce(&mut Renderer) -> Result<(), AppError>,
    ) -> Result<(), FfiError> {
        RENDERER.with_borrow_mut(|slot| {
            let Some(renderer) = slot.as_mut()
            else {
                return Err(FfiError::Failed {
                    message: "no renderer exists on this thread".to_string(),
                });
            };

            body(renderer).map_err(FfiError::from)
        })
    }
}

#[uniffi::export]
pub fn set_cache_directory(cache_directory: String) -> Result<(), FfiError> {
    set_once(&CACHE_DIRECTORY, PathBuf::from(cache_directory))
}

fn set_once(cell: &OnceLock<PathBuf>, path: PathBuf) -> Result<(), FfiError> {
    let set_result: Result<(), PathBuf> = cell.set(path);

    match set_result {
        Ok(()) => Ok(()),
        Err(_rejected_path) => Err(FfiError::Failed {
            message: "the cache directory is already set".to_string(),
        }),
    }
}

fn create_cache() -> Result<FilesystemArtifactCache, FfiError> {
    let cache_directory: Option<&PathBuf> = CACHE_DIRECTORY.get();

    let Some(cache_directory) = cache_directory
    else {
        return Err(FfiError::Failed {
            message: "the cache directory has not been set".to_string(),
        });
    };

    Ok(FilesystemArtifactCache::create(cache_directory.clone()))
}

#[uniffi::export]
pub fn revision() -> String {
    shared::revision::REVISION.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_once_accepts_the_first_path_and_rejects_a_second() {
        let cell: OnceLock<PathBuf> = OnceLock::new();

        let first: Result<(), FfiError> = set_once(&cell, PathBuf::from("/caches/first"));
        let second: Result<(), FfiError> = set_once(&cell, PathBuf::from("/caches/second"));

        assert!(first.is_ok());
        assert!(second.is_err());
        assert_eq!(cell.get(), Some(&PathBuf::from("/caches/first")));
    }
}
