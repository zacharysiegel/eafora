//! The `#[repr(C)]` structs uploaded verbatim into GPU buffers. Their field order, types, and
//! alignment must match what the WGSL shaders read, which is why they carry `bytemuck` derives and
//! fixed-size array fields rather than idiomatic Rust shapes.

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

/// A Miller-projected 2D position.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ProjectedVertex {
    pub position: Vec2,
}

/// A per-vertex fill color; the RGBA channels map to the `Vec4`'s x, y, z, w.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FillVertex {
    pub color: Vec4,
}

/// The projected viewport corners: two `vec2<f32>` at offsets 0 and 8. The 16-byte size is already a
/// multiple of 16, so no padding is needed. The antimeridian wrap is derived in the shader from these
/// bounds (per instance), so it is not stored here.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewportUniform {
    pub projected_min: Vec2,
    pub projected_max: Vec2,
}

const _: () = assert!(std::mem::size_of::<ViewportUniform>() == 16);
