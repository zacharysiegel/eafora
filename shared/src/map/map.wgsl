struct ViewportUniform {
    projected_min: vec2<f32>,
    projected_max: vec2<f32>,
    surface_size: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: ViewportUniform;

// Per-country emphasis, one texel per country: r is the outward lift in screen pixels, g the outline rim
// width. A texture rather than a uniform array because a uniform block holds only 1,024 of these, and a
// layer carrying every subnational level holds more countries than that. Read with textureLoad, which takes
// no sampler; adding one would override the texture's NEAREST filtering and make every read return zero.
@group(0) @binding(1)
var country_state: texture_2d<f32>;

// Must match COUNTRY_STATE_TEXTURE_WIDTH in gpu_types.rs, which the CPU sizes the texture by.
const COUNTRY_STATE_TEXTURE_WIDTH: u32 = 256u;

fn country_state_of(country_index: u32) -> vec4<f32> {
    let texel: vec2<i32> = vec2<i32>(
        i32(country_index % COUNTRY_STATE_TEXTURE_WIDTH),
        i32(country_index / COUNTRY_STATE_TEXTURE_WIDTH),
    );

    return textureLoad(country_state, texel, 0);
}

const PI: f32 = 3.141592653589793;
const TWO_PI: f32 = 6.283185307179586;

// The horizontal shift, in whole turns (2π), applied to the wrapped instance when the viewport
// straddles the ±π antimeridian: -1 if the view has panned off the west edge, +1 off the east edge,
// 0 when the view fits within one world copy. Derived from the bounds so the shader and the renderer's
// instance-count decision share one crossing test. Assumes the viewport is never wider than 2π, so at
// most one seam is crossed.
fn wrap_direction() -> i32 {
    if (viewport.projected_min.x < -PI) { return -1; }
    if (viewport.projected_max.x > PI) { return 1; }
    return 0;
}

// Projects a Miller-projected position into clip space. Instance 0 is the natural copy; instance 1
// (drawn only when the viewport crosses the seam) is shifted a full turn by `wrap_direction` so it
// lands across the antimeridian. Instance 0 is never shifted, since 0 * wrap_direction == 0.
fn project_to_clip(position: vec2<f32>, instance_index: u32) -> vec4<f32> {
    let turns: i32 = i32(instance_index) * wrap_direction();
    let shifted_x: f32 = position.x + f32(turns) * TWO_PI;
    let span: vec2<f32> = viewport.projected_max - viewport.projected_min;
    let normalized_x: f32 = (shifted_x - viewport.projected_min.x) / span.x;
    let normalized_y: f32 = (position.y - viewport.projected_min.y) / span.y;

    return vec4<f32>(normalized_x * 2.0 - 1.0, normalized_y * 2.0 - 1.0, 0.0, 1.0);
}

// Pushes a vertex outward along its boundary outward-direction by its country's lift plus `extra_px`,
// converting screen pixels to projected units via the isotropic projected-units-per-pixel (equal in x
// and y since the viewport shares the surface's aspect). A zero lift and zero extra leave it untouched.
fn emphasis_offset(position: vec2<f32>, outward_direction: vec2<f32>, country_index: u32, extra_px: f32) -> vec2<f32> {
    let lift_px: f32 = country_state_of(country_index).r + extra_px;
    let projected_span_y: f32 = viewport.projected_max.y - viewport.projected_min.y;
    let projected_per_pixel: f32 = projected_span_y / viewport.surface_size.y;
    return position + outward_direction * (lift_px * projected_per_pixel);
}

// Fill pipeline: the choropleth triangles, one flat color per country.

struct FillVertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) outward_direction: vec2<f32>,
    @location(3) country_index: u32,
};

struct FillVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // Flat: the choropleth color is uniform per country, so take the provoking vertex's value rather
    // than interpolate identical corners.
    @location(0) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn fill_vertex_main(input: FillVertexInput, @builtin(instance_index) instance_index: u32) -> FillVertexOutput {
    var output: FillVertexOutput;
    let lifted_position: vec2<f32> = emphasis_offset(input.position, input.outward_direction, input.country_index, 0.0);
    output.clip_position = project_to_clip(lifted_position, instance_index);
    output.color = input.color;
    return output;
}

@fragment
fn fill_fragment_main(input: FillVertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}

// Emphasis-outline pipeline: the selected/hovered country's fill triangles, inflated by an extra `outline.x`
// pixels and painted solid black. Drawn behind the normal fill so only the extra rim shows, giving a
// uniform outline even on multi-island countries (a filled silhouette, not stroked line segments).

@vertex
fn emphasis_outline_vertex_main(input: FillVertexInput, @builtin(instance_index) instance_index: u32) -> @builtin(position) vec4<f32> {
    let outline_px: f32 = country_state_of(input.country_index).g;
    let inflated_position: vec2<f32> = emphasis_offset(input.position, input.outward_direction, input.country_index, outline_px);
    return project_to_clip(inflated_position, instance_index);
}

@fragment
fn emphasis_outline_fragment_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

// Boundary pipeline: each country's boundary as line segments, opaque black.

struct BoundaryVertexInput {
    @location(0) position: vec2<f32>,
    @location(2) outward_direction: vec2<f32>,
    @location(3) country_index: u32,
};

@vertex
fn boundary_vertex_main(input: BoundaryVertexInput, @builtin(instance_index) instance_index: u32) -> @builtin(position) vec4<f32> {
    let lifted_position: vec2<f32> = emphasis_offset(input.position, input.outward_direction, input.country_index, 0.0);
    return project_to_clip(lifted_position, instance_index);
}

@fragment
fn boundary_fragment_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}
