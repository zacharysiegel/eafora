struct ViewportUniform {
    projected_min: vec2<f32>,
    projected_max: vec2<f32>,
    // A signed count of whole 360-degree turns to shift every vertex horizontally, so a second draw
    // can render the wrapped copy of seam-crossing geometry when the viewport straddles the +/-180
    // antimeridian. 0 for the natural draw, +/-1 for a wrapped copy. Only x wraps: latitude does not.
    longitude_wrap_turns: i32,
};

@group(0) @binding(0)
var<uniform> viewport: ViewportUniform;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // Flat: the choropleth color is uniform per country, so take the provoking vertex's value rather
    // than interpolate identical corners.
    @location(0) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let shifted_x: f32 = input.position.x + f32(viewport.longitude_wrap_turns) * 360.0;
    let span: vec2<f32> = viewport.projected_max - viewport.projected_min;
    let normalized_x: f32 = (shifted_x - viewport.projected_min.x) / span.x;
    let normalized_y: f32 = (input.position.y - viewport.projected_min.y) / span.y;

    var output: VertexOutput;
    output.clip_position = vec4<f32>(normalized_x * 2.0 - 1.0, normalized_y * 2.0 - 1.0, 0.0, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
