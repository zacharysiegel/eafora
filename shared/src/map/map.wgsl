struct ViewportUniform {
    projected_min: vec2<f32>,
    projected_max: vec2<f32>,
    surface_size: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: ViewportUniform;

/* Per-country emphasis, one texel per country, in the channel order of the CPU's CountryState. A uniform
   block holds at most 1,024 of these, which a layer carrying every subnational level can exceed. Read with
   textureLoad, which takes no sampler. */
@group(0) @binding(1)
var country_state: texture_2d<f32>;

fn country_state_of(country_index: u32) -> vec4<f32> {
    // Taken from the bound texture so the CPU's row width is the only definition of it.
    let width: u32 = textureDimensions(country_state).x;
    let texel: vec2<i32> = vec2<i32>(
        i32(country_index % width),
        i32(country_index / width),
    );

    return textureLoad(country_state, texel, 0);
}

fn emphasis_lift_px(state: vec4<f32>) -> f32 {
    return state.r;
}

fn emphasis_outline_px(state: vec4<f32>) -> f32 {
    return state.g;
}

const PI: f32 = 3.141592653589793;
const TWO_PI: f32 = 6.283185307179586;

/* The horizontal shift, in whole turns (2π), applied to the wrapped instance when the viewport
   straddles the ±π antimeridian. Assumes the viewport is never wider than 2π. */
fn wrap_direction() -> i32 {
    if (viewport.projected_min.x < -PI) { return -1; }
    if (viewport.projected_max.x > PI) { return 1; }
    return 0;
}

// Projects a Miller-projected position into clip space; instance 1 is the copy shifted a full turn across the antimeridian.
fn project_to_clip(position: vec2<f32>, instance_index: u32) -> vec4<f32> {
    let turns: i32 = i32(instance_index) * wrap_direction();
    let shifted_x: f32 = position.x + f32(turns) * TWO_PI;
    let span: vec2<f32> = viewport.projected_max - viewport.projected_min;
    let normalized_x: f32 = (shifted_x - viewport.projected_min.x) / span.x;
    let normalized_y: f32 = (position.y - viewport.projected_min.y) / span.y;

    return vec4<f32>(normalized_x * 2.0 - 1.0, normalized_y * 2.0 - 1.0, 0.0, 1.0);
}

// Pushes a vertex outward along its boundary outward-direction by its country's lift plus `extra_px`.
fn emphasis_offset(position: vec2<f32>, outward_direction: vec2<f32>, state: vec4<f32>, extra_px: f32) -> vec2<f32> {
    let lift_px: f32 = emphasis_lift_px(state) + extra_px;
    let projected_span_y: f32 = viewport.projected_max.y - viewport.projected_min.y;
    // The viewport shares the surface's aspect, so the y span alone scales both axes.
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
    // The choropleth color is uniform per country.
    @location(0) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn fill_vertex_main(input: FillVertexInput, @builtin(instance_index) instance_index: u32) -> FillVertexOutput {
    var output: FillVertexOutput;
    let state: vec4<f32> = country_state_of(input.country_index);
    let lifted_position: vec2<f32> = emphasis_offset(input.position, input.outward_direction, state, 0.0);
    output.clip_position = project_to_clip(lifted_position, instance_index);
    output.color = input.color;
    return output;
}

@fragment
fn fill_fragment_main(input: FillVertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}

/* Emphasis-outline pipeline: the emphasized country's fill triangles, inflated by its outline width and
   painted solid black. Drawn behind the normal fill, so only the rim shows. */

@vertex
fn emphasis_outline_vertex_main(input: FillVertexInput, @builtin(instance_index) instance_index: u32) -> @builtin(position) vec4<f32> {
    let state: vec4<f32> = country_state_of(input.country_index);
    let inflated_position: vec2<f32> =
        emphasis_offset(input.position, input.outward_direction, state, emphasis_outline_px(state));
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
    let state: vec4<f32> = country_state_of(input.country_index);
    let lifted_position: vec2<f32> = emphasis_offset(input.position, input.outward_direction, state, 0.0);
    return project_to_clip(lifted_position, instance_index);
}

@fragment
fn boundary_fragment_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}
