#import bevy_pbr::forward_io::VertexOutput

struct FlashMaterial {
    color: vec4<f32>,
    alpha: f32,
    camera_pos: vec3<f32>,
    _pad0: f32,
};

@group(3) @binding(0)
var<uniform> material: FlashMaterial;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    let normal = normalize(select(-in.world_normal, in.world_normal, is_front));
    let view_dir = normalize(material.camera_pos - in.world_position.xyz);
    let center = max(dot(normal, view_dir), 0.0);
    let alpha = material.alpha * center * center;
    return vec4(material.color.rgb * alpha, alpha);
}
