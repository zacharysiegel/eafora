//! The `#[repr(C)]` structs uploaded verbatim into GPU buffers. Their field order, types, and
//! alignment must match what the WGSL shaders read, which is why they carry `bytemuck` derives and
//! fixed-size array fields rather than idiomatic Rust shapes.

/// A Miller-projected 2D position.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ProjectedVertex {
    pub position: [f32; 2],
}

/// An RGBA color, one component per channel in `[0, 1]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FillVertex {
    pub color: [f32; 4],
}

/// `bounds` packs the projected viewport as `[min_x, min_y, max_x, max_y]`; `offset[0]` is a
/// horizontal longitude shift. WGSL aligns uniform members to 16 bytes, so packing these as two
/// `vec4`-sized arrays makes the `#[repr(C)]` layout meet that requirement without hand-computed
/// padding.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewportUniform {
    pub bounds: [f32; 4],
    pub offset: [f32; 4],
}
