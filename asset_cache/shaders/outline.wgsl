#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var normal_texture: texture_2d<f32>;

struct OutlineSettings {
    threshold: f32,
    color: vec4<f32>,
}
@group(0) @binding(4) var<uniform> settings: OutlineSettings;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(screen_texture, screen_sampler, in.uv);

    let size = vec2<i32>(textureDimensions(depth_texture));
    let c = vec2<i32>(in.uv * vec2<f32>(size));
    let c10 = min(c + vec2<i32>(1, 0), size - 1);
    let c01 = min(c + vec2<i32>(0, 1), size - 1);
    let c11 = min(c + vec2<i32>(1, 1), size - 1);

    // Roberts cross on depth
    let d00 = textureLoad(depth_texture, c,   0);
    let d10 = textureLoad(depth_texture, c10, 0);
    let d01 = textureLoad(depth_texture, c01, 0);
    let d11 = textureLoad(depth_texture, c11, 0);
    let edge_d = abs(d11 - d00) + abs(d01 - d10);

    // Roberts cross on normals
    let n00 = textureLoad(normal_texture, c,   0).xyz;
    let n10 = textureLoad(normal_texture, c10, 0).xyz;
    let n01 = textureLoad(normal_texture, c01, 0).xyz;
    let n11 = textureLoad(normal_texture, c11, 0).xyz;
    let edge_n = length(n11 - n00) + length(n01 - n10);

    let edge = smoothstep(settings.threshold, settings.threshold * 4.0, edge_d * 10.0 + edge_n);
    return mix(color, vec4<f32>(settings.color.rgb, 1.0), edge * settings.color.a);
}
