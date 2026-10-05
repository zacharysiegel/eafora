use std::cell::RefCell;
use std::future::Future;
use std::sync::Arc;

use tokio::runtime::{Builder, Runtime};
use tokio::sync::watch;

use shared::artifact::Bundle;
use shared::error::AppError;
use shared::map::{Renderer, RendererBackend};

use crate::bundle;
use crate::error::FfiError;
use crate::handle::UiKitSurfaceHandle;

/* wgpu state is bound to its creating thread. Only the synchronous functions below touch this; an async
   export could resume on another thread. */
thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

/// Builds the renderer on the calling thread, which every later renderer function must also use.
/// Requires a published bundle; the renderer reads its geometry at construction.
#[uniffi::export]
pub fn create_renderer() -> Result<(), FfiError> {
    let receiver: watch::Receiver<Arc<Bundle>> = bundle::subscribe()?;

    let created: Result<Renderer, AppError> =
        block_on_calling_thread(Renderer::new(receiver, RendererBackend::Default))?;
    let renderer: Renderer = created?;

    RENDERER.with_borrow_mut(|slot| slot.replace(renderer));

    Ok(())
}

#[uniffi::export]
pub fn attach_surface(handle: UiKitSurfaceHandle, width: u32, height: u32) -> Result<(), FfiError> {
    with_renderer(|renderer| {
        let attached: Result<(), AppError> = block_on_calling_thread(renderer.attach_surface_from_window_handle(
            handle.to_window_handle(),
            width,
            height,
        ))?;

        attached.map_err(FfiError::from)
    })
}

#[uniffi::export]
pub fn resize_surface(width: u32, height: u32) -> Result<(), FfiError> {
    with_renderer(|renderer| renderer.resize_surface(width, height).map_err(FfiError::from))
}

#[uniffi::export]
pub fn detach_surface() -> Result<(), FfiError> {
    with_renderer(|renderer| {
        renderer.detach_surface();

        Ok(())
    })
}

#[uniffi::export]
pub fn destroy_renderer() {
    RENDERER.with_borrow_mut(|slot| slot.take());
}

/// Runs a future to completion without leaving the calling thread.
fn block_on_calling_thread<F: Future>(future: F) -> Result<F::Output, FfiError> {
    let built: Result<Runtime, std::io::Error> = Builder::new_current_thread().build();

    let runtime: Runtime = built.map_err(|error| FfiError::Failed {
        message: format!("building the renderer setup runtime failed; [error={error}]"),
    })?;

    Ok(runtime.block_on(future))
}

fn with_renderer(body: impl FnOnce(&mut Renderer) -> Result<(), FfiError>) -> Result<(), FfiError> {
    RENDERER.with_borrow_mut(|slot| {
        let Some(renderer) = slot.as_mut()
        else {
            return Err(FfiError::Failed {
                message: "no renderer exists on this thread".to_string(),
            });
        };

        body(renderer)
    })
}
