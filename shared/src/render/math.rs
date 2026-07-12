//! General GPU vector types: `#[repr(C)]` and `bytemuck`-derived so they upload verbatim into GPU
//! buffers, matching WGSL's `vec2<f32>` / `vec4<f32>` layout.

/// A 2D `f32` vector, matching WGSL's `vec2<f32>` (8 bytes, components at offsets 0 and 4).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// A 4D `f32` vector, matching WGSL's `vec4<f32>` (16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}
