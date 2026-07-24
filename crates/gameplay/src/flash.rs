#[cfg(feature = "client")]
use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::{color::LinearRgba, pbr::MeshMaterial3d, render::render_resource::AsBindGroup};

#[cfg(feature = "client")]
pub const MIN_VISIBLE_BRIGHTNESS: f32 = 0.02;

#[cfg(feature = "client")]
pub const MIN_VISIBLE_LIGHT_INTENSITY: f32 = 1.0;

#[cfg(feature = "client")]
const MIN_DECAY: f32 = 0.01;

pub struct FlashPlugin;

impl Plugin for FlashPlugin {
    fn build(&self, _app: &mut App) {
        #[cfg(feature = "client")]
        _app.add_plugins(bevy::pbr::MaterialPlugin::<FlashMaterial>::default())
            .add_systems(Update, tick_flashes);
    }
}

pub fn spawn_flash(
    world: &mut World,
    position: Vec3,
    scale: f32,
    color: Color,
    unculled: bool,
    brightness: f32,
    brightness_decay: f32,
    light_intensity: f32,
    light_decay: f32,
    shadows_enabled: bool,
    velocity: Vec3,
) -> Option<Entity> {
    #[cfg(not(feature = "client"))]
    {
        let _ = (
            world,
            position,
            scale,
            color,
            unculled,
            brightness,
            brightness_decay,
            light_intensity,
            light_decay,
            shadows_enabled,
            velocity,
        );
        None
    }
    #[cfg(feature = "client")]
    {
        spawn_flash_client(
            world,
            position,
            scale,
            color,
            unculled,
            brightness,
            brightness_decay,
            light_intensity,
            light_decay,
            shadows_enabled,
            velocity,
        )
    }
}

#[cfg(feature = "client")]
#[derive(Component)]
struct Flash {
    velocity: Vec3,
    color: LinearRgba,
    initial_brightness: f32,
    brightness_decay: f32,
    initial_light_intensity: f32,
    light_decay: f32,
    age_secs: f32,
    brightness_lifetime_secs: f32,
    light_lifetime_secs: f32,
}

#[cfg(feature = "client")]
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[uniform(0, FlashMaterialUniform)]
pub struct FlashMaterial {
    pub params: FlashMaterialUniform,
}

#[cfg(feature = "client")]
#[derive(Reflect, Debug, Clone, bevy::render::render_resource::ShaderType)]
pub struct FlashMaterialUniform {
    pub color: Vec4,
    pub alpha: f32,
    pub camera_pos: Vec3,
    pub _pad0: f32,
}

#[cfg(feature = "client")]
impl From<&FlashMaterial> for FlashMaterialUniform {
    fn from(material: &FlashMaterial) -> Self {
        material.params.clone()
    }
}

#[cfg(feature = "client")]
impl bevy::pbr::Material for FlashMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/flash.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_shadows() -> bool {
        false
    }
}

#[cfg(feature = "client")]
pub fn clamp_decay(decay: f32) -> f32 {
    decay.max(MIN_DECAY)
}

#[cfg(feature = "client")]
pub fn flash_lifetime_secs(initial_value: f32, min_visible: f32, decay: f32) -> f32 {
    if initial_value <= min_visible {
        0.0
    } else {
        (initial_value / min_visible).ln() / clamp_decay(decay)
    }
}

#[cfg(feature = "client")]
pub fn flash_decay(initial_value: f32, decay: f32, age_secs: f32) -> f32 {
    initial_value * (-clamp_decay(decay) * age_secs).exp()
}

#[cfg(feature = "client")]
pub fn spawn_flash_mesh(
    world: &mut World,
    color: LinearRgba,
) -> (Handle<Mesh>, Handle<FlashMaterial>) {
    let mesh = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        meshes.add(bevy::math::primitives::Sphere::new(1.0))
    };
    let material = {
        let mut materials = world.resource_mut::<Assets<FlashMaterial>>();
        materials.add(FlashMaterial {
            params: FlashMaterialUniform {
                color: color.to_vec4(),
                alpha: 0.0,
                camera_pos: Vec3::ZERO,
                _pad0: 0.0,
            },
        })
    };
    (mesh, material)
}

#[cfg(feature = "client")]
pub fn update_flash_material(
    handle: &MeshMaterial3d<FlashMaterial>,
    materials: &mut Assets<FlashMaterial>,
    color: LinearRgba,
    brightness: f32,
    initial_brightness: f32,
    camera_pos: Vec3,
) {
    if let Some(mut material) = materials.get_mut(&handle.0) {
        material.params.color = (color * brightness).to_vec4();
        material.params.alpha = if initial_brightness > 0.0 {
            (brightness / initial_brightness).clamp(0.0, 1.0)
        } else {
            0.0
        };
        material.params.camera_pos = camera_pos;
    }
}

#[cfg(feature = "client")]
fn spawn_flash_client(
    world: &mut World,
    position: Vec3,
    scale: f32,
    color: Color,
    unculled: bool,
    brightness: f32,
    brightness_decay: f32,
    light_intensity: f32,
    light_decay: f32,
    shadows_enabled: bool,
    velocity: Vec3,
) -> Option<Entity> {
    let initial_brightness = brightness.max(0.0);
    let initial_light_intensity = light_intensity.max(0.0);
    if initial_brightness <= MIN_VISIBLE_BRIGHTNESS || scale <= 0.0 {
        return None;
    }
    let brightness_decay = clamp_decay(brightness_decay);
    let light_decay = clamp_decay(light_decay);
    let brightness_lifetime_secs =
        flash_lifetime_secs(initial_brightness, MIN_VISIBLE_BRIGHTNESS, brightness_decay);
    let light_lifetime_secs = flash_lifetime_secs(
        initial_light_intensity,
        MIN_VISIBLE_LIGHT_INTENSITY,
        light_decay,
    );
    let color = color.to_linear();
    let (mesh, material) = spawn_flash_mesh(world, color);

    let mut entity = world.spawn((
        Flash {
            velocity,
            color,
            initial_brightness,
            brightness_decay,
            initial_light_intensity,
            light_decay,
            age_secs: 0.0,
            brightness_lifetime_secs,
            light_lifetime_secs,
        },
        Mesh3d(mesh),
        MeshMaterial3d(material),
        PointLight {
            intensity: initial_light_intensity,
            color: color.into(),
            range: scale * 8.0,
            shadow_maps_enabled: shadows_enabled,
            contact_shadows_enabled: shadows_enabled,
            ..default()
        },
        Transform::from_translation(position).with_scale(Vec3::splat(scale)),
    ));
    if unculled {
        entity.insert(NoFrustumCulling);
    }
    Some(entity.id())
}

#[cfg(feature = "client")]
fn tick_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut flashes: Query<(
        Entity,
        &mut Flash,
        &mut Transform,
        &mut PointLight,
        &bevy::pbr::MeshMaterial3d<FlashMaterial>,
    )>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut materials: ResMut<Assets<FlashMaterial>>,
) {
    let dt = time.delta_secs();
    let camera_pos = camera
        .single()
        .map(GlobalTransform::translation)
        .unwrap_or(Vec3::ZERO);
    for (entity, mut flash, mut transform, mut light, material_handle) in &mut flashes {
        flash.age_secs += dt;
        if flash.age_secs >= flash.brightness_lifetime_secs
            && flash.age_secs >= flash.light_lifetime_secs
        {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += flash.velocity * dt;
        let brightness = flash_decay(
            flash.initial_brightness,
            flash.brightness_decay,
            flash.age_secs,
        );
        light.intensity = flash_decay(
            flash.initial_light_intensity,
            flash.light_decay,
            flash.age_secs,
        );
        light.color = flash.color.into();
        update_flash_material(
            material_handle,
            &mut materials,
            flash.color,
            brightness,
            flash.initial_brightness,
            camera_pos,
        );
    }
}
