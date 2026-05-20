use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::light::NotShadowCaster;
#[cfg(feature = "client")]
use bevy::render::render_resource::AsBindGroup;
use physics::{collider_flags::ColliderFlags, physics_world::PhysicsWorld};
use rapier3d::prelude::{
    Collider, ColliderHandle, Group, InteractionGroups, InteractionTestMode, RigidBodyHandle,
};

#[derive(Component, Clone, Copy)]
pub struct Shield {
    pub current: f32,
    pub max: f32,
    pub regen_per_sec: f32,
    pub regen_delay_secs: f32,
    pub regen_delay_remaining_secs: f32,
    pub enabled: bool,
    pub double_sided: bool,
    pub collider: Option<ColliderHandle>,
    #[cfg(feature = "client")]
    pub visual: Option<Entity>,
}

impl Shield {
    pub fn new(max: f32, regen_per_sec: f32, regen_delay_secs: f32, double_sided: bool) -> Self {
        Self {
            current: max,
            max,
            regen_per_sec,
            regen_delay_secs,
            regen_delay_remaining_secs: 0.0,
            enabled: true,
            double_sided,
            collider: None,
            #[cfg(feature = "client")]
            visual: None,
        }
    }

    pub fn apply_damage(&mut self, damage: f32) -> bool {
        if !self.enabled || damage <= 0.0 {
            return false;
        }
        self.regen_delay_remaining_secs = self.regen_delay_secs;
        self.current = (self.current - damage).max(0.0);
        if self.current <= 0.0 {
            self.enabled = false;
        }
        true
    }
}

pub struct ShieldPlugin;
impl Plugin for ShieldPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "client")]
        app.add_plugins(bevy::pbr::MaterialPlugin::<ShieldMaterial>::default())
            .add_systems(Update, tick_shield_materials);
        app.add_systems(
            FixedUpdate,
            regenerate_shields.in_set(crate::AuthoritySystems),
        );
        app.add_systems(FixedUpdate, sync_shield_colliders);
        #[cfg(feature = "client")]
        app.add_systems(FixedUpdate, sync_shield_visuals);
    }
}

#[cfg(feature = "client")]
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[uniform(0, ShieldMaterialUniform)]
struct ShieldMaterial {
    params: ShieldMaterialUniform,
}

#[cfg(feature = "client")]
#[derive(Reflect, Debug, Clone, bevy::render::render_resource::ShaderType)]
struct ShieldMaterialUniform {
    front_color: Vec4,
    back_color: Vec4,
    edge_alpha: f32,
    min_alpha: f32,
    edge_power: f32,
    camera_pos: Vec3,
    _pad0: Vec3,
    _pad1: f32,
}

#[cfg(feature = "client")]
impl From<&ShieldMaterial> for ShieldMaterialUniform {
    fn from(material: &ShieldMaterial) -> Self {
        material.params.clone()
    }
}

#[cfg(feature = "client")]
impl bevy::pbr::Material for ShieldMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/shield.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
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

pub fn shield_user_data() -> u128 {
    ColliderFlags::SHIELD.bits()
}

pub fn attach_shield_collider(
    body_handle: RigidBodyHandle,
    mut collider: Collider,
    max_health: f32,
    regen_per_sec: f32,
    regen_delay_secs: f32,
    double_sided: bool,
    world: &mut PhysicsWorld,
) -> Shield {
    let no_contacts = InteractionGroups::new(Group::ALL, Group::NONE, InteractionTestMode::And);
    collider.set_density(0.0);
    collider.set_collision_groups(no_contacts);
    collider.set_solver_groups(no_contacts);
    collider.user_data = shield_user_data();
    let handle = world.collider_set.insert_with_parent(
        collider,
        body_handle,
        &mut world.rigid_body_set,
    );
    let mut shield = Shield::new(max_health, regen_per_sec, regen_delay_secs, double_sided);
    shield.collider = Some(handle);
    shield
}

fn regenerate_shields(time: Res<Time<Fixed>>, mut shields: Query<&mut Shield>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut shield in &mut shields {
        shield.regen_delay_remaining_secs = (shield.regen_delay_remaining_secs - dt).max(0.0);
        if shield.current >= shield.max || shield.regen_per_sec <= 0.0 {
            continue;
        }
        if shield.regen_delay_remaining_secs > 0.0 {
            continue;
        }
        shield.current = (shield.current + shield.regen_per_sec * dt).min(shield.max);
        if shield.current > 0.0 {
            shield.enabled = true;
        }
    }
}

pub fn sync_shield_colliders(
    world: ResMut<PhysicsWorld>,
    shields: Query<&Shield, Changed<Shield>>,
) {
    let mut world = world;
    for shield in shields.iter() {
        let Some(handle) = shield.collider else {
            continue;
        };
        let Some(collider) = world.collider_set.get_mut(handle) else {
            continue;
        };
        collider.set_enabled(shield.enabled);
    }
}

#[cfg(feature = "client")]
fn sync_shield_visuals(
    mut visibility_q: Query<&mut Visibility>,
    shields: Query<&Shield, Changed<Shield>>,
) {
    for shield in shields.iter() {
        let Some(visual) = shield.visual else {
            continue;
        };
        let Ok(mut visibility) = visibility_q.get_mut(visual) else {
            continue;
        };
        *visibility = if shield.enabled {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(feature = "client")]
fn tick_shield_materials(
    camera_q: Query<&GlobalTransform, With<Camera3d>>,
    visuals: Query<&bevy::pbr::MeshMaterial3d<ShieldMaterial>>,
    mut materials: ResMut<Assets<ShieldMaterial>>,
) {
    let Ok(camera) = camera_q.single() else {
        return;
    };
    let camera_pos = camera.translation();
    for handle in &visuals {
        let Some(material) = materials.get_mut(&handle.0) else {
            continue;
        };
        material.params.camera_pos = camera_pos;
    }
}

/// example visuals, a simple box
#[cfg(feature = "client")]
pub fn spawn_box_shield_visual(
    world: &mut World,
    parent: Entity,
    half_extents: Vec3,
    double_sided: bool,
) -> Entity {
    let mesh = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        meshes.add(Cuboid::from_size(half_extents * 2.0))
    };
    let color = if double_sided {
        Color::srgba(0.3, 0.78, 1.0, 1.0)
    } else {
        Color::srgba(1.0, 0.24, 0.24, 1.0)
    };
    let back_color = Color::srgba(0.3, 0.78, 1.0, 1.0);
    let material = {
        let mut materials = world.resource_mut::<Assets<ShieldMaterial>>();
        materials.add(ShieldMaterial {
            params: ShieldMaterialUniform {
                front_color: color.to_linear().to_vec4(),
                back_color: back_color.to_linear().to_vec4(),
                edge_alpha: 0.16,
                min_alpha: 0.035,
                edge_power: 2.8,
                camera_pos: Vec3::ZERO,
                _pad0: Vec3::ZERO,
                _pad1: 0.0,
            },
        })
    };
    let visual = world
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            NotShadowCaster,
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    world.entity_mut(parent).add_child(visual);
    visual
}
