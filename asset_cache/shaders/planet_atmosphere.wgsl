#import bevy_pbr::forward_io::VertexOutput

struct PlanetAtmosphereMaterial {
    planet_center: vec3<f32>,
    planet_radius: f32,
    atmosphere_radius: f32,
    density: f32,
    specular: f32,
    opacity: f32,
    color: vec4<f32>,
    sun_color: vec4<f32>,
    sun_dir: vec3<f32>,
    _unused_ambient: f32,
    sun_intensity: f32,
    forward_scatter: f32,
    _unused_steps: u32,
    camera_pos: vec3<f32>,
    _pad0: u32,
};

@group(3) @binding(0)
var<uniform> material: PlanetAtmosphereMaterial;

fn ray_sphere(ro: vec3<f32>, rd: vec3<f32>, center: vec3<f32>, radius: f32) -> vec2<f32> {
    let oc = ro - center;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - radius * radius;
    let h = b * b - c;
    if (h < 0.0) {
        return vec2<f32>(1e20, -1e20);
    }
    let root = sqrt(h);
    return vec2<f32>(-b - root, -b + root);
}

fn phase_rayleigh(cos_theta: f32) -> f32 {
    return 0.75 * (1.0 + cos_theta * cos_theta);
}

fn phase_mie(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = pow(max(1.0 + g2 - 2.0 * g * cos_theta, 1e-3), 1.5);
    return (1.0 - g2) / (12.566371 * denom);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ro = material.camera_pos;
    let rd = normalize(in.world_position.xyz - ro);
    let light_dir = normalize(material.sun_dir);
    let world_pos = in.world_position.xyz;
    let shell = max(material.atmosphere_radius - material.planet_radius, 1e-3);
    let outer_hit = ray_sphere(ro, rd, material.planet_center, material.atmosphere_radius);
    let camera_height = length(ro - material.planet_center);
    let shell_altitude = clamp(
        (material.atmosphere_radius - camera_height) / shell,
        0.0,
        1.0,
    );
    let inside_amount = pow(shell_altitude, 0.6);
    let outside_amount = 1.0 - shell_altitude;

    let up = normalize(world_pos - material.planet_center);
    let view_dir = normalize(ro - world_pos);
    let cos_view_up = dot(view_dir, up);
    let grazing = pow(1.0 - abs(cos_view_up), 1.7);
    let path_length = outer_hit.y - outer_hit.x;
    let thickness = clamp(path_length / shell, 0.0, 8.0);
    let thickness_fade = 1.0 - exp(-thickness * 0.35);

    let cos_theta = dot(rd, light_dir);
    let phase =
        phase_rayleigh(cos_theta) + material.forward_scatter * phase_mie(cos_theta, 0.5);
    let sun_facing = dot(up, -light_dir);
    let sun_visibility = smoothstep(-0.15, 0.35, sun_facing + grazing * 0.35);
    let halo_visibility = smoothstep(0.0, 0.6, sun_facing);
    let reflected_light = reflect(light_dir, up);
    let specular_glow = pow(max(dot(view_dir, reflected_light), 0.0), 24.0);

    let haze = thickness_fade * material.density * 1.4 + inside_amount * material.density * 1.8;
    let rim = grazing * material.density * (thickness_fade * 1.6 + inside_amount * 1.4);
    let halo =
        grazing * specular_glow * material.density * material.sun_intensity * material.specular
        * outside_amount * halo_visibility;
    let alpha = clamp((haze + rim) * material.opacity * sun_visibility + halo * material.opacity, 0.0, 1.0);

    let base = vec3<f32>(0.0);
    let haze_lit = material.color.rgb * haze * sun_visibility;
    let rim_lit =
        material.color.rgb * material.sun_color.rgb * phase * rim * sun_visibility * material.sun_intensity;
    let halo_lit = material.sun_color.rgb * phase * halo;
    return vec4(base + haze_lit + rim_lit + halo_lit, alpha);
}
