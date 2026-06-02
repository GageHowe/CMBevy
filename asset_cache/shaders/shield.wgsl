#import bevy_pbr::forward_io::VertexOutput

struct ShieldMaterial {
    front_color: vec4<f32>,
    back_color: vec4<f32>,
    edge_alpha: f32,
    min_alpha: f32,
    edge_power: f32,
    camera_pos: vec3<f32>,
    _pad0: vec3<f32>,
    _pad1: f32,
};

@group(3) @binding(0)
var<uniform> material: ShieldMaterial;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    let normal = normalize(select(-in.world_normal, in.world_normal, is_front));
    let view_dir = normalize(material.camera_pos - in.world_position.xyz);
    let fresnel = pow(1.0 - max(dot(normal, view_dir), 0.0), material.edge_power);
    let alpha = max(material.min_alpha, material.edge_alpha * fresnel);
    let base_color = select(material.back_color.rgb, material.front_color.rgb, is_front);
    let color = base_color * (0.3 + 0.7 * fresnel);
    return vec4(color, alpha);
}
