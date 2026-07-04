// Packed into vec4s for unambiguous 16-byte uniform alignment: bounds.xy = projected min,
// bounds.zw = projected max, offset.x = the horizontal shift for antimeridian wraparound draws.
struct ViewportUniform {
    bounds: vec4<f32>,
    offset: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: ViewportUniform;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let projected_min: vec2<f32> = viewport.bounds.xy;
    let projected_max: vec2<f32> = viewport.bounds.zw;
    let shifted_x: f32 = input.position.x + viewport.offset.x;
    let span: vec2<f32> = projected_max - projected_min;
    let normalized_x: f32 = (shifted_x - projected_min.x) / span.x;
    let normalized_y: f32 = (input.position.y - projected_min.y) / span.y;

    var output: VertexOutput;
    output.clip_position = vec4<f32>(normalized_x * 2.0 - 1.0, normalized_y * 2.0 - 1.0, 0.0, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
