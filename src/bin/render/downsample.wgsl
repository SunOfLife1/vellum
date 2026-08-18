// Resolve a supersampled render target into the Wayland surface.
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

@group(0) @binding(0)
var source: texture_2d<f32>;

@group(0) @binding(1)
var source_sampler: sampler;

struct ResolveInfo {
    origin: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(2)
var<uniform> resolve: ResolveInfo;

override exact: bool;
override render_scale: u32;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = array(
        vec2(-1.0, -1.0),
        vec2(3.0, -1.0),
        vec2(-1.0, 3.0),
    )[index];
    var output: VertexOutput;
    output.position = vec4(position, 0.0, 1.0);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(source));
    let pixel = floor(input.position.xy - resolve.origin);
    let base = pixel * f32(render_scale);
    if exact {
        var color = vec4(0.0);
        for (var y = 0u; y < render_scale; y++) {
            for (var x = 0u; x < render_scale; x++) {
                color += textureLoad(
                    source,
                    vec2<i32>(base) + vec2<i32>(i32(x), i32(y)),
                    0,
                );
            }
        }
        return color / f32(render_scale * render_scale);
    }
    let offsets = array(-0.6666667, 1.0, 2.6666667);
    let weights = array(6.0, 20.0, 6.0);
    var color = vec4(0.0);
    for (var y = 0; y < 3; y++) {
        for (var x = 0; x < 3; x++) {
            let uv = (base + vec2(offsets[x], offsets[y])) / dimensions;
            color += textureSampleLevel(source, source_sampler, uv, 0.0)
                * weights[x] * weights[y];
        }
    }
    return color / 1024.0;
}
