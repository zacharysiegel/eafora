//! The `#[repr(C)]` structs uploaded verbatim into GPU buffers. Their field order, types, and
//! alignment must match what the WGSL shaders read, which is why they carry `bytemuck` derives and
//! fixed-size array fields rather than idiomatic Rust shapes.

use crate::map::color::Rgba;

/// A 2D `f32` vector, matching WGSL's `vec2<f32>` (8 bytes, components at offsets 0 and 4).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// A Miller-projected 2D position.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ProjectedVertex {
    pub position: Vec2,
}

/// A per-vertex fill color.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FillVertex {
    pub color: Rgba,
}

/// The projected viewport corners plus a horizontal longitude shift. `_padding` brings the size to
/// 32 bytes (a multiple of 16); the field offsets (0, 8, 16) match the `vec2, vec2, f32` the shaders
/// declare for this uniform.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewportUniform {
    pub projected_min: Vec2,
    pub projected_max: Vec2,
    pub longitude_offset: f32,
    pub _padding: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<ViewportUniform>() == 32);
