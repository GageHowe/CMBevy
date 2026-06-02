#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct ColorCompressionSettings {
    color_steps: f32,
    dither_strength: f32,
};

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: ColorCompressionSettings;

fn bayer(pixel: vec2<f32>) -> f32 {
    let x = i32(pixel.x) % 4;
    let y = i32(pixel.y) % 4;
    let idx = y * 4 + x;
    const LUT: array<f32, 16> = array<f32, 16>(
        0.0/16.0,  8.0/16.0,  2.0/16.0, 10.0/16.0,
        12.0/16.0, 4.0/16.0, 14.0/16.0, 6.0/16.0,
        3.0/16.0, 11.0/16.0, 1.0/16.0,  9.0/16.0,
        15.0/16.0, 7.0/16.0, 13.0/16.0, 5.0/16.0,
    );
    return LUT[idx] - 0.5;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(screen_texture, screen_sampler, in.uv);
    let steps = max(settings.color_steps, 2.0);
    let steps_minus_one = steps - 1.0;
    let step_size = 1.0 / steps_minus_one;
    let pattern = bayer(in.position.xy) * step_size * settings.dither_strength;
    let biased = clamp(base.rgb + pattern, vec3(0.0), vec3(1.0));
    let compressed = round(biased * steps_minus_one) / steps_minus_one;
    return vec4(compressed, base.a);
}
