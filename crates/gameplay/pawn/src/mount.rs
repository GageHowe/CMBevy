use bevy::prelude::*;
use net::{
    message::NetworkID,
    quic::{ConnectionId, QuicManager, SendTarget},
};
#[cfg(feature = "client")]
use physics::physics_world::sync_physics_visual;
use physics::physics_world::{ForceApplication, PhysicsWorld, rb_angvel, rb_pos, rb_rot, rb_vel};

#[cfg(feature = "client")]
use crate::NetworkEntityMap;
#[cfg(feature = "client")]
use crate::pawn::Possessed;

/// Occupancy data for a parent object that can attach a character at a fixed anchor.
#[derive(Component, Reflect)]
pub struct CharacterMount {
    /// Character currently attached to this mount, if any.
    pub occupant: Option<Entity>,
    /// Child entity whose transform defines the rider attach point.
    pub anchor: Entity,
    /// World-space interaction radius used by aim-based enter tests.
    pub interact_radius: f32,
    /// Local-space offset used when dismounting.
    pub exit_offset: Vec3,
}

/// Marker on a rider telling systems which parent object currently owns its pose.
#[derive(Component, Clone, Copy, Reflect)]
pub struct Mounted(pub Entity);

/// Shared plugin for generic character mounting and visual/physics sync.
pub struct MountPlugin;
impl Plugin for MountPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<CharacterMount>();
        app.register_type::<Mounted>();
        app.add_systems(FixedUpdate, sync_mounted_bipeds.in_set(ForceApplication));
        #[cfg(feature = "client")]
        app.add_systems(
            Update,
            sync_mounted_biped_visuals.after(sync_physics_visual),
        );
    }
}

pub fn spawn_mount_anchor(parent: Entity, local_offset: Vec3, world: &mut World) -> Entity {
    let anchor = world
        .spawn((
            Transform::from_translation(local_offset),
            Visibility::default(),
        ))
        .id();
    world.entity_mut(parent).add_child(anchor);
    anchor
}

pub fn mount_world_point(parent_pos: Vec3, parent_rot: Quat, anchor_local: Vec3) -> Vec3 {
    parent_pos + parent_rot * anchor_local
}

pub fn ray_hits_mount(
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
    center: Vec3,
    radius: f32,
) -> Option<f32> {
    let offset = center - origin;
    let along = offset.dot(dir);
    if along < 0.0 || along > max_distance {
        return None;
    }
    let closest = origin + dir * along;
    (center.distance_squared(closest) <= radius * radius).then_some(along)
}

pub fn anchor_transform<'a>(
    mount: &CharacterMount,
    anchor_transforms: &'a Query<&Transform>,
) -> Option<&'a Transform> {
    anchor_transforms.get(mount.anchor).ok()
}

pub fn mount_character(
    world: &mut PhysicsWorld,
    biped_entity: Entity,
    parent_entity: Entity,
    mount: &mut CharacterMount,
    anchor_transform: &Transform,
) -> bool {
    if mount.occupant.is_some() {
        return false;
    }
    let Some(&parent_handle) = world.entity_to_handle.get(&parent_entity) else {
        return false;
    };
    let Some(parent_body) = world.rigid_body_set.get(parent_handle) else {
        return false;
    };
    let parent_pos = rb_pos(parent_body);
    let parent_rot = rb_rot(parent_body);
    let parent_vel = rb_vel(parent_body);
    let parent_angvel = rb_angvel(parent_body);
    let mount_pos = mount_world_point(parent_pos, parent_rot, anchor_transform.translation);
    let mount_rot = parent_rot * anchor_transform.rotation;
    world.set_body_pose(
        biped_entity,
        mount_pos,
        mount_rot,
        parent_vel,
        parent_angvel,
    );
    world.set_body_enabled(biped_entity, false);
    mount.occupant = Some(biped_entity);
    true
}

pub fn unmount_character(
    world: &mut PhysicsWorld,
    parent_entity: Entity,
    mount: &mut CharacterMount,
    anchor_transform: &Transform,
) -> Option<Entity> {
    let biped_entity = mount.occupant.take()?;
    let exit_offset = anchor_transform.rotation * mount.exit_offset + anchor_transform.translation;
    let (exit_pos, parent_rot, exit_vel, _) =
        world.predicted_body_point_after(parent_entity, exit_offset, 0.0)?;
    world.set_body_enabled(biped_entity, true);
    world.set_body_pose(biped_entity, exit_pos, parent_rot, exit_vel, Vec3::ZERO);
    Some(biped_entity)
}

pub fn mount_in_range(
    world: &PhysicsWorld,
    character: Entity,
    parent: Entity,
    mount: &CharacterMount,
    anchor_transform: &Transform,
) -> bool {
    let character_pos = world
        .entity_to_handle
        .get(&character)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(rb_pos);
    let mount_pos = world
        .entity_to_handle
        .get(&parent)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| mount_world_point(rb_pos(rb), rb_rot(rb), anchor_transform.translation));
    matches!((character_pos, mount_pos), (Some(a), Some(b)) if {
        let d = a - b;
        d.x * d.x + d.y * d.y + d.z * d.z
            < (mount.interact_radius + 4.0) * (mount.interact_radius + 4.0)
    })
}

pub fn try_mount_character(
    world: &mut PhysicsWorld,
    character: Entity,
    parent: Entity,
    mount: &mut CharacterMount,
    anchor_transforms: &Query<&Transform>,
) -> bool {
    let Some(anchor_transform) = anchor_transform(mount, anchor_transforms) else {
        return false;
    };
    mount_character(world, character, parent, mount, anchor_transform)
}

pub fn try_unmount_character(
    world: &mut PhysicsWorld,
    parent: Entity,
    mount: &mut CharacterMount,
    anchor_transforms: &Query<&Transform>,
) -> Option<Entity> {
    let anchor_transform = anchor_transform(mount, anchor_transforms)?;
    unmount_character(world, parent, mount, anchor_transform)
}

pub fn can_mount_character(
    world: &PhysicsWorld,
    character: Entity,
    parent: Entity,
    mount: &CharacterMount,
    anchor_transforms: &Query<&Transform>,
) -> bool {
    let Some(anchor_transform) = anchor_transform(mount, anchor_transforms) else {
        return false;
    };
    mount_in_range(world, character, parent, mount, anchor_transform)
}

pub enum MountInteractResult {
    Mounted,
    Unmounted(Entity),
}

pub fn handle_mount_interact(
    controlled: Entity,
    character: Entity,
    parent: Entity,
    world: &mut PhysicsWorld,
    mount: &mut CharacterMount,
    anchor_transforms: &Query<&Transform>,
) -> Option<MountInteractResult> {
    if mount.occupant.is_some() && controlled == parent {
        return try_unmount_character(world, parent, mount, anchor_transforms)
            .map(MountInteractResult::Unmounted);
    }
    if mount.occupant.is_some()
        || !can_mount_character(world, character, parent, mount, anchor_transforms)
    {
        return None;
    }
    try_mount_character(world, character, parent, mount, anchor_transforms)
        .then_some(MountInteractResult::Mounted)
}

pub fn handle_server_interact(
    conn_id: ConnectionId,
    controlled: Entity,
    character: Entity,
    character_net_id: &NetworkID,
    target: Entity,
    target_net_id: &NetworkID,
    registry: &mut crate::pawn::PlayerRegistry,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    net_ids: &Query<&NetworkID>,
    mounts: &mut Query<&mut CharacterMount>,
    anchor_transforms: &Query<&Transform>,
    commands: &mut Commands,
) {
    let Ok(mut mount) = mounts.get_mut(target) else {
        return;
    };
    match handle_mount_interact(
        controlled,
        character,
        target,
        world,
        &mut mount,
        anchor_transforms,
    ) {
        Some(MountInteractResult::Unmounted(biped_entity)) => {
            let Ok(biped_net_id) = net_ids.get(biped_entity) else {
                return;
            };
            commands.entity(biped_entity).remove::<Mounted>();
            crate::pawn::possess_pawn(conn_id, biped_entity, biped_net_id, registry, quic);
            crate::pawn::send_mount_state(quic, SendTarget::All, biped_net_id, None);
        }
        Some(MountInteractResult::Mounted) => {
            commands.entity(character).insert(Mounted(target));
            crate::pawn::possess_pawn(conn_id, target, target_net_id, registry, quic);
            crate::pawn::send_mount_state(
                quic,
                SendTarget::All,
                character_net_id,
                Some(target_net_id),
            );
        }
        None => {}
    }
}

#[cfg(feature = "client")]
pub fn apply_mount_state(
    biped_net_id: &net::message::NetworkID,
    parent_net_id: Option<&net::message::NetworkID>,
    local_net_id: Option<&net::message::NetworkID>,
    just_spawned: &std::collections::HashMap<net::message::NetworkID, (Entity, u64)>,
    networked: &NetworkEntityMap,
    interaction_names: &Query<&crate::interaction::InteractionName>,
    mounted: &Query<&Mounted>,
    mounts: &Query<&CharacterMount>,
    anchor_transforms: &Query<&Transform>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let biped_entity = just_spawned
        .get(biped_net_id)
        .map(|(entity, _)| *entity)
        .or_else(|| networked.get_entity(biped_net_id));
    let Some(biped_entity) = biped_entity else {
        return;
    };
    match parent_net_id.and_then(|id| networked.get_entity(id)) {
        Some(parent_entity) => {
            world.set_body_enabled(biped_entity, false);
            commands.entity(biped_entity).insert(Mounted(parent_entity));
            if local_net_id == Some(biped_net_id)
                && let Ok(name) = interaction_names.get(parent_entity)
            {
                crate::messages::push(commands, format!("Entered {}", name.0));
            }
        }
        None => {
            let old_parent = mounted.get(biped_entity).ok().map(|mounted| mounted.0);
            let predicted_exit = old_parent.and_then(|parent_entity| {
                mounts.get(parent_entity).ok().and_then(|mount| {
                    let anchor_transform = anchor_transforms.get(mount.anchor).ok()?;
                    let exit_offset = anchor_transform.rotation * mount.exit_offset
                        + anchor_transform.translation;
                    world.predicted_body_point_after(parent_entity, exit_offset, 0.0)
                })
            });
            if let Some((pos, rot, vel, _)) = predicted_exit {
                world.set_body_enabled(biped_entity, true);
                world.set_body_pose(biped_entity, pos, rot, vel, Vec3::ZERO);
            } else {
                world.set_body_enabled(biped_entity, true);
            }
            if let Some(&handle) = world.entity_to_handle.get(&biped_entity)
                && let Some(rb) = world.rigid_body_set.get_mut(handle)
            {
                rb.set_angvel(physics::physics_world::Vector3::ZERO, true);
            }
            commands.entity(biped_entity).remove::<Mounted>();
            if local_net_id == Some(biped_net_id)
                && let Some(parent_entity) = old_parent
                && let Ok(name) = interaction_names.get(parent_entity)
            {
                crate::messages::push(commands, format!("Exited {}", name.0));
            }
        }
    }
}

fn sync_mounted_bipeds(
    mut world: ResMut<PhysicsWorld>,
    mounted: Query<(Entity, &Mounted)>,
    mounts: Query<&CharacterMount>,
    anchor_transforms: Query<&Transform>,
) {
    for (biped_entity, mounted) in mounted.iter() {
        let Some(parent_handle) = world.entity_to_handle.get(&mounted.0).copied() else {
            continue;
        };
        let Some(parent_body) = world.rigid_body_set.get(parent_handle) else {
            continue;
        };
        let Ok(mount) = mounts.get(mounted.0) else {
            continue;
        };
        let Ok(anchor_transform) = anchor_transforms.get(mount.anchor) else {
            continue;
        };
        let parent_pos = rb_pos(parent_body);
        let parent_rot = rb_rot(parent_body);
        let mount_pos = mount_world_point(parent_pos, parent_rot, anchor_transform.translation);
        let mount_rot = parent_rot * anchor_transform.rotation;
        let parent_vel = rb_vel(parent_body);
        let parent_angvel = rb_angvel(parent_body);
        world.set_body_pose(
            biped_entity,
            mount_pos,
            mount_rot,
            parent_vel,
            parent_angvel,
        );
    }
}

#[cfg(feature = "client")]
fn sync_mounted_biped_visuals(
    mounted: Query<(Entity, &Mounted)>,
    mut transforms: ParamSet<(Query<&Transform>, Query<&mut Transform>)>,
    mounts: Query<&CharacterMount>,
) {
    for (biped_entity, mounted) in mounted.iter() {
        let (parent_translation, parent_rotation, anchor_translation, anchor_rotation) = {
            let parents = transforms.p0();
            let Ok(parent_transform) = parents.get(mounted.0) else {
                continue;
            };
            let Ok(mount) = mounts.get(mounted.0) else {
                continue;
            };
            let Ok(anchor_transform) = parents.get(mount.anchor) else {
                continue;
            };
            (
                parent_transform.translation,
                parent_transform.rotation,
                anchor_transform.translation,
                anchor_transform.rotation,
            )
        };
        let mut bipeds = transforms.p1();
        let Ok(mut biped_transform) = bipeds.get_mut(biped_entity) else {
            continue;
        };
        biped_transform.translation = parent_translation + parent_rotation * anchor_translation;
        biped_transform.rotation = parent_rotation * anchor_rotation;
    }
}

#[cfg(feature = "client")]
pub fn draw_mount_debug(
    mounts: Query<&CharacterMount>,
    anchors: Query<&GlobalTransform>,
    mut gizmos: Gizmos,
) {
    for mount in mounts.iter() {
        let Ok(gt) = anchors.get(mount.anchor) else {
            continue;
        };
        let (_, rot, center) = gt.to_scale_rotation_translation();
        let color = if mount.occupant.is_some() {
            Color::srgba(1.0, 0.2, 0.2, 0.9)
        } else {
            Color::srgba(0.2, 1.0, 0.8, 0.9)
        };
        gizmos.sphere(center, mount.interact_radius, color);
        gizmos.line(
            center,
            center + rot * mount.exit_offset,
            Color::srgba(1.0, 0.8, 0.2, 0.9),
        );
    }
}

pub fn handle_mount_parent_death(parent_entity: Entity, world: &mut World) -> Option<Entity> {
    let Some(anchor) = world
        .get::<CharacterMount>(parent_entity)
        .map(|mount| mount.anchor)
    else {
        return None;
    };
    let Some(anchor_transform) = world.get::<Transform>(anchor).cloned() else {
        return None;
    };
    let biped_entity = world.resource_scope(|world, mut physics: Mut<PhysicsWorld>| {
        let Some(mut mount) = world.get_mut::<CharacterMount>(parent_entity) else {
            return None;
        };
        unmount_character(&mut physics, parent_entity, &mut mount, &anchor_transform)
    })?;
    world.entity_mut(biped_entity).remove::<Mounted>();
    Some(biped_entity)
}

#[cfg(feature = "client")]
pub fn clear_mount_possession(parent_entity: Entity, rider_entity: Entity, world: &mut World) {
    if world.get::<Possessed>(parent_entity).is_some() {
        crate::pawn::detach_camera(world);
        world.entity_mut(parent_entity).remove::<Possessed>();
        world.entity_mut(rider_entity).insert(Possessed::new(128));
    }
}
