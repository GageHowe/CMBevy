/*
atmosphere-adjacent zones:
* area reverb drives client audio based on listener proximity
*/

use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::{color::LinearRgba, pbr::MeshMaterial3d, render::render_resource::AsBindGroup};
use serde::{Deserialize, Serialize};

pub struct AtmospherePlugin;
impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<AreaReverbComponent>()
            .register_type::<PlanetAtmosphere>();

        #[cfg(feature = "client")]
        app.add_plugins(bevy::pbr::MaterialPlugin::<PlanetAtmosphereMaterial>::default())
            .add_systems(
                Update,
                (
                    spawn_planet_atmospheres,
                    cleanup_planet_atmospheres,
                    sync_planet_atmosphere_shells,
                ),
            );
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct AreaReverbComponent {
    /// full-effect radius
    pub min_distance: f32,
    /// fade-out radius
    pub max_distance: f32,
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct PlanetAtmosphere {
    /// Radius of the solid body in world units.
    pub planet_radius: f32,
    /// Additional shell thickness above `planet_radius`.
    pub shell_thickness: f32,
    /// Main scattering tint.
    pub color: Color,
    /// Overall shell density. Higher values make the atmosphere thicker.
    pub density: f32,
    /// Extra alpha multiplier after integration.
    pub opacity: f32,
    /// Extra directional light gain.
    pub sun_intensity: f32,
    /// Forward-scattering amount for sun glints. `0` disables the warm forward lobe.
    pub forward_scatter: f32,
    /// Extra sun glint on the exterior limb.
    pub specular: f32,
}

impl Default for PlanetAtmosphere {
    fn default() -> Self {
        Self {
            planet_radius: 100.0,
            shell_thickness: 3.0,
            color: Color::srgb(0.35, 0.55, 1.0),
            density: 0.08,
            opacity: 1.0,
            sun_intensity: 15.0,
            forward_scatter: 0.15,
            specular: 2.0,
        }
    }
}

#[cfg(feature = "client")]
#[derive(Component)]
struct PlanetAtmosphereShell;

#[cfg(feature = "client")]
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[uniform(0, PlanetAtmosphereMaterialUniform)]
#[reflect(Default)]
struct PlanetAtmosphereMaterial {
    params: PlanetAtmosphereMaterialUniform,
}

#[cfg(feature = "client")]
impl Default for PlanetAtmosphereMaterial {
    fn default() -> Self {
        Self {
            params: PlanetAtmosphereMaterialUniform::default(),
        }
    }
}

#[cfg(feature = "client")]
#[derive(Reflect, Debug, Clone, bevy::render::render_resource::ShaderType)]
struct PlanetAtmosphereMaterialUniform {
    planet_center: Vec3,
    planet_radius: f32,
    atmosphere_radius: f32,
    density: f32,
    specular: f32,
    opacity: f32,
    color: Vec4,
    sun_color: Vec4,
    sun_dir: Vec3,
    _unused_ambient: f32,
    sun_intensity: f32,
    forward_scatter: f32,
    _unused_steps: u32,
    camera_pos: Vec3,
    _pad0: u32,
}

#[cfg(feature = "client")]
impl Default for PlanetAtmosphereMaterialUniform {
    fn default() -> Self {
        Self {
            planet_center: Vec3::ZERO,
            planet_radius: 100.0,
            atmosphere_radius: 103.0,
            density: 0.08,
            specular: 2.0,
            opacity: 1.0,
            color: Vec4::new(0.35, 0.55, 1.0, 1.0),
            sun_color: Vec4::ONE,
            sun_dir: Vec3::Y,
            _unused_ambient: 0.0,
            sun_intensity: 15.0,
            forward_scatter: 0.15,
            _unused_steps: 0,
            camera_pos: Vec3::ZERO,
            _pad0: 0,
        }
    }
}

#[cfg(feature = "client")]
impl From<&PlanetAtmosphereMaterial> for PlanetAtmosphereMaterialUniform {
    fn from(material: &PlanetAtmosphereMaterial) -> Self {
        material.params.clone()
    }
}

#[cfg(feature = "client")]
impl From<&PlanetAtmosphere> for PlanetAtmosphereMaterial {
    fn from(atmosphere: &PlanetAtmosphere) -> Self {
        let mut material = Self::default();
        apply_planet_atmosphere_component(&mut material.params, atmosphere);
        material
    }
}

#[cfg(feature = "client")]
impl bevy::pbr::Material for PlanetAtmosphereMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/planet_atmosphere.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[cfg(feature = "client")]
fn spawn_planet_atmospheres(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PlanetAtmosphereMaterial>>,
    atmospheres: Query<(Entity, &PlanetAtmosphere), Added<PlanetAtmosphere>>,
) {
    for (entity, atmosphere) in &atmospheres {
        let shell = commands
            .spawn((
                PlanetAtmosphereShell,
                Mesh3d(meshes.add(bevy::math::primitives::Sphere::new(1.0))),
                MeshMaterial3d(materials.add(PlanetAtmosphereMaterial::from(atmosphere))),
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        commands.entity(entity).add_child(shell);
    }
}

#[cfg(feature = "client")]
fn cleanup_planet_atmospheres(
    mut commands: Commands,
    shells: Query<(Entity, &ChildOf), With<PlanetAtmosphereShell>>,
    parents: Query<(), With<PlanetAtmosphere>>,
) {
    for (entity, child_of) in &shells {
        if parents.get(child_of.parent()).is_err() {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(feature = "client")]
fn sync_planet_atmosphere_shells(
    parents: Query<(&PlanetAtmosphere, &GlobalTransform)>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<PlanetAtmosphereShell>)>,
    lights: Query<(&DirectionalLight, &GlobalTransform)>,
    mut shells: Query<
        (
            &ChildOf,
            &MeshMaterial3d<PlanetAtmosphereMaterial>,
            &mut Transform,
        ),
        With<PlanetAtmosphereShell>,
    >,
    mut materials: ResMut<Assets<PlanetAtmosphereMaterial>>,
) {
    let camera_pos = camera
        .single()
        .map(GlobalTransform::translation)
        .unwrap_or(Vec3::ZERO);
    let (sun_dir, sun_color) = brightest_directional_light(&lights);

    for (child_of, material_handle, mut shell_transform) in &mut shells {
        let Ok((atmosphere, parent_transform)) = parents.get(child_of.parent()) else {
            continue;
        };
        let outer_radius = atmosphere.planet_radius + atmosphere.shell_thickness.max(0.001);
        let parent_scale = parent_transform.to_scale_rotation_translation().0;
        shell_transform.scale = Vec3::new(
            safe_axis_scale(outer_radius, parent_scale.x),
            safe_axis_scale(outer_radius, parent_scale.y),
            safe_axis_scale(outer_radius, parent_scale.z),
        );

        let Some(material) = materials.get_mut(&material_handle.0) else {
            continue;
        };
        apply_planet_atmosphere_component(&mut material.params, atmosphere);
        material.params.planet_center = parent_transform.translation();
        material.params.camera_pos = camera_pos;
        material.params.sun_dir = sun_dir;
        material.params.sun_color = sun_color;
    }
}

#[cfg(feature = "client")]
fn apply_planet_atmosphere_component(
    params: &mut PlanetAtmosphereMaterialUniform,
    atmosphere: &PlanetAtmosphere,
) {
    let color = LinearRgba::from(atmosphere.color).to_vec4();
    params.planet_radius = atmosphere.planet_radius.max(0.001);
    params.atmosphere_radius = (atmosphere.planet_radius + atmosphere.shell_thickness.max(0.001))
        .max(params.planet_radius + 0.001);
    params.density = atmosphere.density.max(0.0);
    params.specular = atmosphere.specular.max(0.0);
    params.opacity = atmosphere.opacity.clamp(0.0, 4.0);
    params.color = color;
    params.sun_intensity = atmosphere.sun_intensity.max(0.0);
    params.forward_scatter = atmosphere.forward_scatter.clamp(0.0, 1.0);
}

#[cfg(feature = "client")]
fn brightest_directional_light(
    lights: &Query<(&DirectionalLight, &GlobalTransform)>,
) -> (Vec3, Vec4) {
    lights
        .iter()
        .max_by(|(a, _), (b, _)| a.illuminance.total_cmp(&b.illuminance))
        .map(|(light, transform)| {
            (
                transform.forward().as_vec3(),
                LinearRgba::from(light.color).to_vec4(),
            )
        })
        .unwrap_or((Vec3::Y, Vec4::ONE))
}

#[cfg(feature = "client")]
fn safe_axis_scale(radius: f32, parent_scale: f32) -> f32 {
    if parent_scale.abs() > 1e-4 {
        radius / parent_scale.abs()
    } else {
        radius
    }
}
