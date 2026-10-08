use std::cell::RefCell;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::thread::{self, ThreadId};

use tokio::runtime::{Builder, Runtime};
use tokio::sync::watch;

use shared::artifact::Bundle;
use shared::error::AppError;
use shared::map::{Renderer, RendererBackend};

use crate::bundle;
use crate::error::FfiError;
use crate::handle::UiKitSurfaceHandle;

// The first thread to call a renderer function owns the renderer for the life of the process.
static RENDERER_THREAD: OnceLock<ThreadId> = OnceLock::new();

/* wgpu state is bound to its creating thread, so only the synchronous functions below touch this. UniFFI
   polls an `async fn` exported with `#[uniffi::export]` on a multi-threaded runtime, so it may continue on a
   different thread after each `.await`. */
thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

/// Builds the renderer on the calling thread, which every later renderer function must also use.
/// Requires a published bundle; the renderer reads its geometry at construction.
#[uniffi::export]
pub fn create_renderer() -> Result<(), FfiError> {
    require_renderer_thread()?;

    let renderer_exists: bool = RENDERER.with_borrow(|slot| slot.is_some());
    if renderer_exists {
        return Err(FfiError::Failed {
            message: "a renderer already exists".to_string(),
        });
    }

    let receiver: watch::Receiver<Arc<Bundle>> = bundle::subscribe()?;

    let created: Result<Renderer, AppError> =
        block_on_calling_thread(Renderer::new(receiver, RendererBackend::Default))?;
    let renderer: Renderer = created?;

    RENDERER.with_borrow_mut(|slot| slot.replace(renderer));

    Ok(())
}

#[uniffi::export]
pub fn attach_surface(handle: UiKitSurfaceHandle, width: u32, height: u32) -> Result<(), FfiError> {
    Ok(with_renderer(|renderer| {
        block_on_calling_thread(renderer.attach_surface_from_window_handle(
            handle.to_window_handle(),
            width,
            height,
        ))?
    })?)
}

#[uniffi::export]
pub fn resize_surface(width: u32, height: u32) -> Result<(), FfiError> {
    Ok(with_renderer(|renderer| renderer.resize_surface(width, height))?)
}

#[uniffi::export]
pub fn detach_surface() -> Result<(), FfiError> {
    Ok(with_renderer(|renderer| {
        renderer.detach_surface();

        Ok(())
    })?)
}

#[uniffi::export]
pub fn destroy_renderer() -> Result<(), FfiError> {
    require_renderer_thread()?;

    RENDERER.with_borrow_mut(|slot| slot.take());

    Ok(())
}

/// Runs a future to completion on the calling thread.
fn block_on_calling_thread<F: Future>(future: F) -> Result<F::Output, AppError> {
    let built: Result<Runtime, std::io::Error> = Builder::new_current_thread().build();

    let runtime: Runtime = built.map_err(|error| {
        AppError::from(format!("building the renderer setup runtime failed; [error={error}]"))
    })?;

    Ok(runtime.block_on(future))
}

/// Records the calling thread as the renderer's owner on the first call, and refuses any other thread.
fn require_renderer_thread() -> Result<(), AppError> {
    let calling_thread: ThreadId = thread::current().id();
    let owning_thread: &ThreadId = RENDERER_THREAD.get_or_init(|| calling_thread);

    if *owning_thread != calling_thread {
        return Err(AppError::from("the renderer belongs to another thread".to_string()));
    }

    Ok(())
}

fn with_renderer(body: impl FnOnce(&mut Renderer) -> Result<(), AppError>) -> Result<(), AppError> {
    require_renderer_thread()?;

    RENDERER.with_borrow_mut(|slot| {
        let Some(renderer) = slot.as_mut()
        else {
            return Err(AppError::from("no renderer exists".to_string()));
        };

        body(renderer)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_renderer_thread_refuses_every_thread_after_the_first() {
        let first_call: Result<(), AppError> = require_renderer_thread();
        let repeated_call: Result<(), AppError> = require_renderer_thread();
        let other_thread_refused: bool = thread::spawn(|| require_renderer_thread().is_err())
            .join()
            .expect("the other thread does not panic");

        assert!(first_call.is_ok());
        assert!(repeated_call.is_ok());
        assert!(other_thread_refused);
    }
}
