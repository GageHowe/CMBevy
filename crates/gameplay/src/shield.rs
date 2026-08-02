#[cfg(feature = "client")]
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::render::render_resource::AsBindGroup;
#[cfg(feature = "client")]
use common::game_state::GameState;
use common::{NetworkID, NetworkIDResource, tick::Ticker};
use physics::{
    collider_flags::ColliderFlags,
    physics_world::{PhysicsWorld, RigidBodyHandleComponent},
};
use rapier3d::prelude::{
    Collider, ColliderBuilder, ColliderHandle, Group, InteractionGroups, InteractionTestMode,
    RigidBodyHandle,
};

use crate::{
    archetype::Archetype, find_entity_by_net_id, health::Health, net::message::SpawnCommand,
};

const SPACESHIP_SHIELD_MAX_HEALTH: i32 = 300;
const SPACESHIP_SHIELD_REGEN_PER_SECOND: i32 = 60;
const SPACESHIP_SHIELD_REGEN_DELAY_TICKS: u16 = common::config::FIXED_TICK_RATE as u16 * 5;
pub const SPACESHIP_SHIELD_HALF_EXTENTS: Vec3 = Vec3::new(10.0, 8.0, 20.0);

#[derive(Component, Clone, Copy)]
pub struct Shield {
    pub double_sided: bool,
    pub collider: Option<ColliderHandle>,
    #[cfg(feature = "client")]
    pub visual: Option<Entity>,
}

impl Shield {
    pub fn new(double_sided: bool) -> Self {
        Self {
            double_sided,
            collider: None,
            #[cfg(feature = "client")]
            visual: None,
        }
    }
}

pub struct ShieldPlugin;
impl Plugin for ShieldPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "client")]
        app.add_plugins(bevy::pbr::MaterialPlugin::<ShieldMaterial>::default())
            .add_systems(
                Update,
                tick_shield_materials.in_set(common::game_state::SimulationSystems),
            );
        app.add_systems(
            FixedUpdate,
            sync_shield_colliders.in_set(common::game_state::SimulationSystems),
        );
        #[cfg(feature = "client")]
        app.add_systems(
            FixedUpdate,
            sync_shield_visuals.in_set(common::game_state::SimulationSystems),
        );
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

pub fn attach_shield_collider(
    body_handle: RigidBodyHandle,
    mut collider: Collider,
    double_sided: bool,
    world: &mut PhysicsWorld,
) -> Shield {
    let no_contacts = InteractionGroups::new(Group::ALL, Group::NONE, InteractionTestMode::And);
    collider.set_density(0.0);
    collider.set_collision_groups(no_contacts);
    collider.set_solver_groups(no_contacts);
    collider.user_data = ColliderFlags::SHIELD.bits();
    let handle =
        world
            .collider_set
            .insert_with_parent(collider, body_handle, &mut world.rigid_body_set);
    let mut shield = Shield::new(double_sided);
    shield.collider = Some(handle);
    shield
}

fn shield_enabled(health: &Health) -> bool {
    !health.is_dead()
}

pub fn sync_shield_colliders(
    world: ResMut<PhysicsWorld>,
    shields: Query<(&Shield, &Health), Or<(Changed<Shield>, Changed<Health>)>>,
) {
    let mut world = world;
    for (shield, health) in shields.iter() {
        let Some(handle) = shield.collider else {
            continue;
        };
        let Some(collider) = world.collider_set.get_mut(handle) else {
            continue;
        };
        collider.set_enabled(shield_enabled(health));
    }
}

#[cfg(feature = "client")]
fn sync_shield_visuals(
    mut visibility_q: Query<&mut Visibility>,
    shields: Query<(&Shield, &Health), Or<(Changed<Shield>, Changed<Health>)>>,
) {
    for (shield, health) in shields.iter() {
        let Some(visual) = shield.visual else {
            continue;
        };
        let Ok(mut visibility) = visibility_q.get_mut(visual) else {
            continue;
        };
        *visibility = if shield_enabled(health) {
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
        let Some(mut material) = materials.get_mut(&handle.0) else {
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

#[derive(Component, Default, Reflect)]
pub struct SpaceshipShieldComponent;

impl crate::archetype::SpawnArchetypeTrait for crate::archetype::SpaceshipShield {
    fn spawn(self, entity: Entity, bundle: crate::archetype::SpawnBundle, world: &mut World) {
        let Some(parent_net_id) = bundle.parent_net_id.as_ref() else {
            return;
        };
        let Some(parent) = find_entity_by_net_id(world, parent_net_id) else {
            return;
        };
        let Some(body_handle) = world
            .get::<RigidBodyHandleComponent>(parent)
            .map(|handle| handle.0)
        else {
            return;
        };
        #[allow(unused_mut)]
        let mut shield = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            attach_shield_collider(
                body_handle,
                ColliderBuilder::cuboid(
                    SPACESHIP_SHIELD_HALF_EXTENTS.x,
                    SPACESHIP_SHIELD_HALF_EXTENTS.y,
                    SPACESHIP_SHIELD_HALF_EXTENTS.z,
                )
                .build(),
                false,
                &mut physics,
            )
        };
        #[cfg(feature = "client")]
        {
            shield.visual = Some(spawn_box_shield_visual(
                world,
                entity,
                SPACESHIP_SHIELD_HALF_EXTENTS,
                shield.double_sided,
            ));
        }
        world.entity_mut(entity).insert((
            crate::SpawnReplicated("spaceship_shield"),
            SpaceshipShieldComponent,
            Health::new(
                SPACESHIP_SHIELD_MAX_HEALTH,
                SPACESHIP_SHIELD_REGEN_PER_SECOND,
                SPACESHIP_SHIELD_REGEN_DELAY_TICKS,
            ),
            shield,
            Transform::default(),
        ));
        crate::insert_spawn_metadata(entity, world, Some(300.0), false, None, false);
    }
}

fn should_spawn_local_shield(_world: &World) -> bool {
    #[cfg(feature = "client")]
    {
        _world
            .get_resource::<State<GameState>>()
            .is_none_or(|state| *state.get() != GameState::Multiplayer)
    }
    #[cfg(not(feature = "client"))]
    {
        true
    }
}

pub fn spawn_attached_spaceship_shield(
    parent_net_id: &NetworkID,
    world: &mut World,
) -> Option<Entity> {
    if !should_spawn_local_shield(world) {
        return None;
    }
    let tick = world
        .get_resource::<Ticker>()
        .map_or(0, |ticker| ticker.tick);
    let net_id = NetworkID(world.get_resource_mut::<NetworkIDResource>()?.next());
    let entity = world.spawn_empty().id();
    let cmd = SpawnCommand::new(
        net_id.clone(),
        Archetype::SpaceshipShield(crate::archetype::SpaceshipShield),
        tick,
    )
    .parent(parent_net_id.clone())
    .position(Vec3::ZERO)
    .rotation(Quat::IDENTITY);
    crate::SpawnGameObjectCommand {
        entity,
        cmd: cmd.clone(),
    }
    .apply(world);
    #[cfg(not(feature = "client"))]
    if let Some(mut quic) = world.get_resource_mut::<crate::net::quic::QuicManager>() {
        quic.send(
            crate::net::quic::SendTarget::All,
            crate::net::quic::Channel::Ordered,
            &crate::net::message::MsgType::SpawnCommand(cmd),
        );
    }
    Some(entity)
}
