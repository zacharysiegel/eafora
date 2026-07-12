struct ViewportUniform {
    projected_min: vec2<f32>,
    projected_max: vec2<f32>,
    longitude_offset: f32,
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
    // longitude_offset shifts every vertex horizontally by a whole turn (+/-360 in projected x) so a
    // second draw can render the wrapped copy of seam-crossing geometry when the viewport straddles
    // the +/-180 antimeridian. It is 0 for the natural draw. Only x wraps: latitude has no wraparound.
    let shifted_x: f32 = input.position.x + viewport.longitude_offset;
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
