struct ViewportUniform {
    projected_min: vec2<f32>,
    projected_max: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: ViewportUniform;

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

// Fill pipeline: the choropleth triangles, one flat color per country.

struct FillVertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
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
    output.clip_position = project_to_clip(input.position, instance_index);
    output.color = input.color;
    return output;
}

@fragment
fn fill_fragment_main(input: FillVertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}

// Border pipeline: the country outlines as line segments, opaque black.

@vertex
fn border_vertex_main(@location(0) position: vec2<f32>, @builtin(instance_index) instance_index: u32) -> @builtin(position) vec4<f32> {
    return project_to_clip(position, instance_index);
}

@fragment
fn border_fragment_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}
