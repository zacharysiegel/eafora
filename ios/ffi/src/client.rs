use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::watch;

use shared::artifact::{load, Bundle, FilesystemArtifactCache};
use shared::http::ReqwestHttpFetch;
use shared::license::DistributionContext;
use shared::map::{Renderer, RendererBackend};
use shared::AppError;

use crate::distribution::FfiDistributionContext;
use crate::error::FfiError;
use crate::handle::UiKitSurfaceHandle;

/* The renderer is held here rather than in `EaforaClient` because wgpu state is bound to the thread that
   created it, and UniFFI requires every exported object to be Send + Sync. The web client holds its driver
   the same way. Only the synchronous functions below touch it, so they run on whichever thread Swift calls
   from; an async export would be free to resume on a worker thread. */
thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

/// The published bundle, once one has been opened. The renderer subscribes to the receiver, so a later live
/// load repaints by sending rather than by rebuilding anything.
struct BundlePublication {
    sender: watch::Sender<Arc<Bundle>>,
    receiver: watch::Receiver<Arc<Bundle>>,
}

#[derive(uniffi::Object)]
pub struct EaforaClient {
    cache: FilesystemArtifactCache,
    http_fetch: ReqwestHttpFetch,
    distribution_context: DistributionContext,
    publication: Mutex<Option<BundlePublication>>,
    /// Drives the renderer's asynchronous setup from whichever thread calls in, since those functions must
    /// not resume elsewhere.
    renderer_setup_runtime: tokio::runtime::Runtime,
}

#[uniffi::export(async_runtime = "tokio")]
impl EaforaClient {
    #[uniffi::constructor]
    pub fn new(
        cache_directory: String,
        distribution_context: FfiDistributionContext,
    ) -> Result<EaforaClient, FfiError> {
        let http_fetch: ReqwestHttpFetch = ReqwestHttpFetch::create()?;
        let renderer_setup_runtime: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|error| FfiError::Failed {
                message: format!("building the renderer setup runtime failed; [error={error}]"),
            })?;

        Ok(EaforaClient {
            cache: FilesystemArtifactCache::create(PathBuf::from(cache_directory)),
            http_fetch,
            distribution_context: DistributionContext::from(distribution_context),
            publication: Mutex::new(None),
            renderer_setup_runtime,
        })
    }

    /// Opens the newest readable cached bundle, falling back to the files shipped in the app bundle.
    pub async fn open_first_paint_bundle(&self, embedded_base_url: String) -> Result<String, FfiError> {
        let cached: Option<Bundle> =
            load::open_newest_cached_bundle(&self.cache, self.distribution_context).await?;

        let bundle: Bundle = match cached {
            Some(cached) => cached,
            None => {
                load::load_embedded_bundle(
                    &self.cache,
                    &self.http_fetch,
                    &embedded_base_url,
                    self.distribution_context,
                )
                .await?
            }
        };

        Ok(self.publish(bundle))
    }

    /// Fetches the newest published bundle and republishes it, repainting through the channel the renderer
    /// already holds.
    pub async fn load_live_bundle(
        &self,
        discovery_url: String,
        static_repository_base_url: String,
    ) -> Result<String, FfiError> {
        let bundle: Bundle = load::load_live_bundle(
            &self.cache,
            &self.http_fetch,
            &discovery_url,
            &static_repository_base_url,
            self.distribution_context,
        )
        .await?;

        let version_label: String = self.publish(bundle);

        let evicted: Result<(), AppError> = load::evict_stale_versions(&self.cache).await;
        if let Err(error) = evicted {
            log::warn!("evicting old cached bundle versions failed; [error={error}]");
        }

        Ok(version_label)
    }

    /// Builds the renderer on the calling thread, which every later renderer function must also run on.
    /// Requires a published bundle, since the renderer reads its geometry at construction.
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
    /// Replaces the published bundle, creating the channel on the first call, and answers with the version
    /// label that is now live.
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
pub fn revision() -> String {
    shared::revision::REVISION.to_string()
}
