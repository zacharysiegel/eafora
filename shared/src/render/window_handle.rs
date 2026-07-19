/// Platform window/layer pointers marshaled from the native shell. No `Wasm` variant: the web path
/// attaches its surface from an `HtmlCanvasElement` directly rather than through a window handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowHandle {
    UiKit { layer_ptr: u64, view_ptr: u64 },
    AndroidNdk { native_window_ptr: u64 },
}
