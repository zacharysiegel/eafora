use std::cell::RefCell;
use std::ffi::c_int;
use std::future::Future;
use std::sync::Arc;

use tokio::runtime::{self, Runtime};
use tokio::sync::watch;

use shared::artifact::Bundle;
use shared::error::AppError;
use shared::map::{Renderer, RendererBackend};

use crate::bundle;
use crate::error::FfiError;
use crate::handle::UiKitSurfaceHandle;

/* wgpu state is bound to its creating thread, so only the synchronous functions below touch this, and only
   on the main thread. UniFFI polls an `async fn` exported with `#[uniffi::export]` on a multi-threaded
   runtime, so it may continue on a different thread after each `.await`. */
thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

/// Builds the renderer on the calling thread, which every later renderer function must also use.
/// Requires a published bundle; the renderer reads its geometry at construction.
#[uniffi::export]
pub fn create_renderer() -> Result<(), FfiError> {
    require_main_thread()?;

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
    require_main_thread()?;

    RENDERER.with_borrow_mut(|slot| slot.take());

    Ok(())
}

/// Runs a future to completion on the calling thread.
fn block_on_calling_thread<F: Future>(future: F) -> Result<F::Output, AppError> {
    let built: Result<Runtime, std::io::Error> = runtime::Builder::new_current_thread().build();

    let runtime: Runtime = built.map_err(|error| {
        AppError::from(format!("building the renderer setup runtime failed; [error={error}]"))
    })?;

    Ok(runtime.block_on(future))
}

fn require_main_thread() -> Result<(), AppError> {
    if !is_main_thread() {
        return Err(AppError::from("renderer functions must be called on the main thread".to_string()));
    }

    Ok(())
}

fn is_main_thread() -> bool {
    // Nonzero on the process's main thread.
    let main_thread_flag: c_int = unsafe { libc::pthread_main_np() };

    main_thread_flag != 0
}

/// Runs `body` with the renderer, which a thread-local lends only within a closure. Requires the main thread and
/// a created renderer.
fn with_renderer(body: impl FnOnce(&mut Renderer) -> Result<(), AppError>) -> Result<(), AppError> {
    require_main_thread()?;

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

    /// The test harness runs each test on a spawned thread, so only the refusal can be tested here.
    #[test]
    fn require_main_thread_refuses_a_spawned_thread() {
        let spawned_thread_refused: bool = std::thread::spawn(|| require_main_thread().is_err())
            .join()
            .expect("the spawned thread does not panic");

        assert!(spawned_thread_refused);
    }
}
