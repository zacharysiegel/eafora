// Packed into vec4s for unambiguous 16-byte uniform alignment: bounds.xy = projected min,
// bounds.zw = projected max, offset.x = the horizontal shift for antimeridian wraparound draws.
struct ViewportUniform {
    bounds: vec4<f32>,
    offset: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: ViewportUniform;

fn project_to_clip(position: vec2<f32>) -> vec4<f32> {
    let projected_min: vec2<f32> = viewport.bounds.xy;
    let projected_max: vec2<f32> = viewport.bounds.zw;
    let shifted_x: f32 = position.x + viewport.offset.x;
    let span: vec2<f32> = projected_max - projected_min;
    let normalized_x: f32 = (shifted_x - projected_min.x) / span.x;
    let normalized_y: f32 = (position.y - projected_min.y) / span.y;

    return vec4<f32>(normalized_x * 2.0 - 1.0, normalized_y * 2.0 - 1.0, 0.0, 1.0);
}

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> @builtin(position) vec4<f32> {
    return project_to_clip(position);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    // Border color: opaque black.
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}
