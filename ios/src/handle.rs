use shared::render::WindowHandle;

/// The layer and view pointers Swift reads off a UIKit view.
#[derive(uniffi::Record)]
pub struct UiKitSurfaceHandle {
    pub layer_ptr: u64,
    pub view_ptr: u64,
}

impl UiKitSurfaceHandle {
    pub fn to_window_handle(&self) -> WindowHandle {
        WindowHandle::UiKit {
            layer_ptr: self.layer_ptr,
            view_ptr: self.view_ptr,
        }
    }
}
