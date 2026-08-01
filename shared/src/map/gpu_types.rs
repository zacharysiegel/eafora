//! The map's `#[repr(C)]` GPU-buffer structs. Their field order, types, and alignment must match
//! what the WGSL shaders read, which is why they carry `bytemuck` derives.

use crate::render::gpu_types::{Vec2, Vec4};

/// A Miller-projected 2D position.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ProjectedVertexAttributes {
    pub position: Vec2,
}

/// A per-vertex fill color; the RGBA channels map to the `Vec4`'s x, y, z, w.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FillVertexAttributes {
    pub color: Vec4,
}

/// Per-vertex input for raising/outlining a country: the vertex shader looks the country's state up by
/// index and pushes the vertex along `outward_direction` to inflate it outward. A separate buffer from
/// the static `positions` and from `FillVertexAttributes`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EmphasisVertexAttributes {
    pub outward_direction: Vec2,
    pub country_index: u32,
}

/// Padded to a multiple of 16 bytes. The antimeridian wrap is derived in the shader from the bounds
/// per instance, so it is not stored here.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewportUniform {
    pub projected_min: Vec2,
    pub projected_max: Vec2,
    /// The render surface's size in physical pixels. The shader uses it to convert the lift and outline
    /// widths (given in pixels) into projected-space distances.
    pub surface_size: Vec2,
    pub _padding: Vec2,
}

const _: () = assert!(std::mem::size_of::<ViewportUniform>() == 32);

/// Per-country emphasis state, indexed by `EmphasisVertexAttributes::country_index` in a uniform
/// array. Padded to a multiple of 16 bytes to match the std140 uniform-array element stride.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CountryState {
    /// Outward lift in screen pixels (0 unless hovered).
    pub lift_px: f32,
    /// Black outline rim width in screen pixels (0 unless hovered or selected).
    pub outline_px: f32,
    pub _padding: [f32; 2],
}

const _: () = assert!(std::mem::size_of::<CountryState>() == 16);

/// The fixed length of the per-country state uniform array; the shader's `array<CountryState, ...>`
/// literal length must match this. At least the number of countries in the layer.
pub const COUNTRY_STATE_ARRAY_LEN: usize = 512;
