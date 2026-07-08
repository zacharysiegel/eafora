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

/// The projected viewport corners plus a horizontal longitude shift. `_padding` brings the size to
/// 32 bytes (a multiple of 16); the field offsets (0, 8, 16) match the `vec2, vec2, f32` the shaders
/// declare for this uniform.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewportUniform {
    pub projected_min: [f32; 2],
    pub projected_max: [f32; 2],
    pub longitude_offset: f32,
    pub _padding: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<ViewportUniform>() == 32);
