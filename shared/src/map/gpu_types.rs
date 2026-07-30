//! The map's `#[repr(C)]` GPU-buffer structs. Their field order, types, and alignment must match
//! what the WGSL shaders read, which is why they carry `bytemuck` derives.

use crate::render::gpu_types::{Vec2, Vec4};

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

/// Per-vertex highlight input: the vertex shader looks the country's highlight state up by index and
/// pushes the vertex outward along the normal by a screen-space amount. A separate buffer from the
/// static `positions` and from `FillVertex`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HighlightVertex {
    pub normal: Vec2,
    pub country_index: u32,
}

/// Frame-global rendering parameters shared by every vertex shader. Padded to 32 bytes (a multiple of
/// 16). The antimeridian wrap is derived in the shader from the bounds (per instance), so it is not
/// stored here.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewportUniform {
    pub projected_min: Vec2,
    pub projected_max: Vec2,
    /// Physical pixels; converts a screen-pixel highlight offset to a projected length.
    pub surface_size: Vec2,
    pub _padding: Vec2,
}

const _: () = assert!(std::mem::size_of::<ViewportUniform>() == 32);

/// Per-country highlight state, indexed by `HighlightVertex::country_index` in a uniform array. Padded
/// to 16 bytes to match the std140 uniform-array element stride.
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

/// The fixed length of the per-country state uniform array (`array<CountryState, 512>` in map.wgsl; the
/// literal there must match this). At least the number of countries in the layer; the bind group's
/// min_binding_size check catches a mismatch with the shader.
pub const COUNTRY_STATE_CAP: usize = 512;
