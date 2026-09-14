use shared::render::WindowHandle;

/// The pointers a UIKit shell hands across the boundary. A record rather than a mirror of `WindowHandle`,
/// whose other variant names a platform this crate is never built for.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_window_handle_carries_both_pointers_into_the_uikit_variant() {
        let handle: UiKitSurfaceHandle = UiKitSurfaceHandle {
            layer_ptr: 0x0123_4567_89ab_cdef,
            view_ptr: 0xfedc_ba98_7654_3210,
        };

        let window_handle: WindowHandle = handle.to_window_handle();

        assert_eq!(
            window_handle,
            WindowHandle::UiKit {
                layer_ptr: 0x0123_4567_89ab_cdef,
                view_ptr: 0xfedc_ba98_7654_3210,
            },
        );
    }
}
