use wgpu::{Device, Surface, SurfaceConfiguration, TextureFormat};

/// A configured wgpu surface plus the `SurfaceConfiguration` it was built with, so `resize` and the
/// reconfigure-on-lost path can reapply a tweaked config without re-deriving it from the adapter's
/// capabilities each time.
pub struct WgpuSurface {
    inner: Surface<'static>,
    config: SurfaceConfiguration,
}

impl WgpuSurface {
    pub fn inner(&self) -> &Surface<'static> {
        &self.inner
    }

    pub fn format(&self) -> TextureFormat {
        self.config.format
    }

    pub fn resize(&mut self, device: &Device, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;

        self.inner.configure(device, &self.config);
    }

    pub fn reconfigure(&self, device: &Device) {
        self.inner.configure(device, &self.config);
    }
}

// not for wasm32: raw-window-handle targets native window systems; the web attaches from a canvas
// (a path not yet implemented), so nothing on wasm32 constructs a surface this way.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use core::ffi::c_void;
    use core::ptr::NonNull;

    use raw_window_handle::{
        AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle, UiKitDisplayHandle,
        UiKitWindowHandle,
    };
    use wgpu::{
        Adapter, Device, Instance, PresentMode, Surface, SurfaceCapabilities, SurfaceColorSpace, SurfaceConfiguration,
        SurfaceTargetUnsafe, TextureUsages,
    };

    use super::WgpuSurface;
    use crate::error::AppError;
    use crate::map::value_types::WindowHandle;

    impl WgpuSurface {
        pub fn from_window_handle(
            instance: &Instance,
            adapter: &Adapter,
            device: &Device,
            window_handle: WindowHandle,
            width: u32,
            height: u32,
        ) -> Result<WgpuSurface, AppError> {
            let target: SurfaceTargetUnsafe = window_handle_to_target(window_handle)?;

            // SAFETY: the layer/view pointers come from the platform shell and, per the FFI
            // contract, are valid and outlive the surface built from them.
            let surface: Surface<'static> = unsafe { instance.create_surface_unsafe(target) }
                .map_err(|error| AppError::from(format!("creating a surface from the window handle failed: {error}")))?;

            let capabilities: SurfaceCapabilities = surface.get_capabilities(adapter);
            let config: SurfaceConfiguration = SurfaceConfiguration {
                usage: TextureUsages::RENDER_ATTACHMENT,
                format: capabilities.formats[0],
                color_space: SurfaceColorSpace::Auto,
                width,
                height,
                present_mode: PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: capabilities.alpha_modes[0],
                view_formats: vec![],
            };

            surface.configure(device, &config);

            Ok(WgpuSurface { inner: surface, config })
        }
    }

    fn window_handle_to_target(window_handle: WindowHandle) -> Result<SurfaceTargetUnsafe, AppError> {
        let (raw_window_handle, raw_display_handle): (RawWindowHandle, RawDisplayHandle) = match window_handle {
            WindowHandle::UiKit { layer_ptr: _, view_ptr } => {
                let ui_view: NonNull<c_void> = NonNull::new(view_ptr as *mut c_void)
                    .ok_or_else(|| AppError::from("the UiKit view pointer was null".to_string()))?;

                (
                    RawWindowHandle::UiKit(UiKitWindowHandle::new(ui_view)),
                    RawDisplayHandle::UiKit(UiKitDisplayHandle::new()),
                )
            }
            WindowHandle::AndroidNdk { native_window_ptr } => {
                let native_window: NonNull<c_void> = NonNull::new(native_window_ptr as *mut c_void)
                    .ok_or_else(|| AppError::from("the Android native window pointer was null".to_string()))?;

                (
                    RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(native_window)),
                    RawDisplayHandle::Android(AndroidDisplayHandle::new()),
                )
            }
        };

        Ok(SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display_handle),
            raw_window_handle,
        })
    }
}
