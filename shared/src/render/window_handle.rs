/// Platform window/layer pointers marshaled from the native shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowHandle {
    UiKit { layer_ptr: u64, view_ptr: u64 },
    AndroidNdk { native_window_ptr: u64 },
}
