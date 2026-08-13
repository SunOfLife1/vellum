// Draw tessellated annotations in screen coordinates.
struct Uniform {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> info: Uniform;

struct VertexInput {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

override target_is_srgb: bool;

// Vertex colors are stored as sRGB; blending and filtering operate in linear space.
fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    return select(
        color / 12.92,
        pow((color + 0.055) / 1.055, vec3(2.4)),
        color > vec3(0.04045),
    );
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.pos = vec4<f32>(
        (in.pos.x * 2.0) / info.screen_size.x - 1.0,
        1.0 - (in.pos.y * 2.0) / info.screen_size.y,
        0.0, 1.0,
    );
    out.color = vec4(
        select(in.color.rgb, srgb_to_linear(in.color.rgb), target_is_srgb),
        in.color.a,
    );
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
