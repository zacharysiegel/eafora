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

fn project_to_clip(position: vec2<f32>) -> vec4<f32> {
    let projected_min: vec2<f32> = viewport.projected_min;
    let projected_max: vec2<f32> = viewport.projected_max;
    let shifted_x: f32 = position.x + f32(viewport.longitude_wrap_turns) * 360.0;
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
