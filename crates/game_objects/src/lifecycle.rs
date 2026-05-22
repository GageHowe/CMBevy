use bevy::prelude::*;
use common::GameObjectKind;
use net::message::{NetworkID, NetworkIDResource, SpawnCommand};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_angvel, rb_pos, rb_vel};

#[cfg(feature = "client")]
use crate::NetworkEntityMap;
use crate::{
    SpawnGameObjectCommand, dispatch_game_object_on_death,
    level::{SpawnPoint, parent_body_handle, parented_world_pose},
};

pub fn spawn_game_object(
    kind: GameObjectKind,
    position: Vec3,
    rotation: Quat,
    starting_velocity: Vec3,
    server_tick: u64,
    commands: &mut Commands,
    net_ids: &mut NetworkIDResource,
) -> (Entity, NetworkID, SpawnCommand) {
    let net_id = NetworkID(net_ids.next());
    let cmd = SpawnCommand {
        net_id: net_id.clone(),
        parent_net_id: None,
        position,
        starting_velocity,
        shooter_velocity: Vec3::ZERO,
        rotation,
        server_tick,
        kind,
    };
    let entity = commands.spawn_empty().id();
    queue_spawn_command_on(entity, cmd.clone(), commands);
    (entity, net_id, cmd)
}

pub fn queue_spawn_command(cmd: SpawnCommand, commands: &mut Commands) -> (Entity, NetworkID, u64) {
    let net_id = cmd.net_id.clone();
    let server_tick = cmd.server_tick;
    let entity = commands.spawn_empty().id();
    queue_spawn_command_on(entity, cmd, commands);
    (entity, net_id, server_tick)
}

pub fn queue_spawn_command_on(entity: Entity, cmd: SpawnCommand, commands: &mut Commands) {
    commands.queue(SpawnGameObjectCommand { entity, cmd });
}

#[cfg(feature = "client")]
pub fn apply_spawn_command(
    commands: &mut Commands,
    networked: &NetworkEntityMap,
    just_spawned: &mut std::collections::HashMap<NetworkID, (Entity, u64)>,
    mut cmd: SpawnCommand,
    rtt_secs: f32,
) {
    cmd.position += cmd.starting_velocity * (rtt_secs / 2.0);
    let net_id = cmd.net_id.clone();
    let server_tick = cmd.server_tick;
    let entity = if let Some(entity) = networked.get(&net_id) {
        queue_spawn_command_on(entity, cmd, commands);
        entity
    } else {
        let (entity, _, _) = queue_spawn_command(cmd, commands);
        entity
    };
    just_spawned.insert(net_id, (entity, server_tick));
}

#[cfg(feature = "client")]
pub fn apply_possess(
    net_id: NetworkID,
    local_net_id: &mut Option<NetworkID>,
    just_spawned: &std::collections::HashMap<NetworkID, (Entity, u64)>,
    networked: &NetworkEntityMap,
    possessed_q: &Query<(Entity, &NetworkID), With<crate::pawn::Possessed>>,
    commands: &mut Commands,
    ticker: &mut common::tick::Ticker,
) {
    *local_net_id = Some(net_id.clone());
    let result = just_spawned
        .get(&net_id)
        .copied()
        .or_else(|| networked.get(&net_id).map(|entity| (entity, ticker.tick)));
    let Some((entity, server_tick)) = result else {
        return;
    };
    for (old, _) in possessed_q.iter() {
        if old != entity {
            commands.entity(old).remove::<crate::pawn::Possessed>();
        }
    }
    ticker.tick = server_tick;
    commands
        .entity(entity)
        .insert(crate::pawn::Possessed::new(128));
}

#[cfg(feature = "client")]
pub fn apply_despawn(
    net_id: &NetworkID,
    local_net_id: &mut Option<NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut bevy::ecs::system::ParamSet<(
        Query<
            (
                &mut crate::pawn::WeaponSlots,
                &crate::pawn::biped::BipedPawnComponent,
            ),
            With<crate::pawn::Possessed>,
        >,
        Query<&crate::pawn::biped::BipedPawnComponent>,
        Query<&mut crate::pawn::biped::BipedPawnComponent>,
    )>,
    commands: &mut Commands,
) {
    let Some(entity) = networked.get(net_id) else {
        return;
    };
    let is_local = local_net_id.as_ref() == Some(net_id);
    if is_local {
        if let Ok(cam) = camera.single()
            && let Ok(mut entity) = commands.get_entity(cam)
        {
            entity.remove_parent_in_place();
        }
        if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
            let held: Vec<_> = slots.held_entities().collect();
            for ent in held {
                if let Ok(mut entity) = commands.get_entity(ent) {
                    entity.queue_silenced(|entity: EntityWorldMut| {
                        entity.despawn();
                        Ok::<(), bevy::ecs::world::error::EntityMutableFetchError>(())
                    });
                }
            }
            slots.clear();
        }
        *local_net_id = None;
    } else if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
        let removed_local = slots.contains_net_id(net_id);
        let removed_active = slots.active().0.as_ref() == Some(net_id);
        slots.remove_by_net_id(net_id);
        if removed_local {
            crate::weapon::helpers::set_local_slot_visibility(commands, &slots);
        }
        if removed_active {
            commands.queue(|world: &mut World| {
                let mut camera =
                    world.query_filtered::<&mut crate::pawn::CameraEffector, With<Camera3d>>();
                if let Ok(mut camera) = camera.single_mut(world) {
                    camera.reset_zoom();
                }
            });
        }
    }
    commands.queue(move |world: &mut World| {
        if !world.entities().contains(entity) {
            return;
        }
        if let Some(kind) = world.get::<GameObjectKind>(entity).cloned()
            && world
                .get::<crate::health::Health>(entity)
                .is_some_and(crate::health::Health::is_dead)
        {
            dispatch_game_object_on_death(kind, entity, world);
        }
        if world.entities().contains(entity) {
            world.entity_mut(entity).despawn();
        }
    });
}

pub fn pick_spawn_point(
    spawn_points: &Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: &Query<&Transform>,
    parent_parents: &Query<&ChildOf>,
    parent_bodies: &Query<&RigidBodyHandleComponent>,
    physics: &PhysicsWorld,
    team: u8,
    counter: usize,
) -> Option<(Vec3, Quat)> {
    pick_spawn_point_with_velocity(
        spawn_points,
        parent_transforms,
        parent_parents,
        parent_bodies,
        physics,
        team,
        counter,
    )
    .map(|(position, rotation, _)| (position, rotation))
}

pub fn pick_spawn_point_with_velocity(
    spawn_points: &Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: &Query<&Transform>,
    parent_parents: &Query<&ChildOf>,
    parent_bodies: &Query<&RigidBodyHandleComponent>,
    physics: &PhysicsWorld,
    team: u8,
    counter: usize,
) -> Option<(Vec3, Quat, Vec3)> {
    let resolved: Vec<_> = spawn_points
        .iter()
        .filter(|(_, sp, _, _)| sp.team == team)
        .filter_map(|(_, _, transform, child_of)| {
            resolve_spawn_point(
                transform,
                child_of,
                parent_transforms,
                parent_parents,
                parent_bodies,
                physics,
            )
        })
        .collect();
    if resolved.is_empty() {
        return None;
    }
    Some(resolved[counter % resolved.len()])
}

fn resolve_spawn_point(
    transform: &Transform,
    child_of: Option<&ChildOf>,
    parent_transforms: &Query<&Transform>,
    parent_parents: &Query<&ChildOf>,
    parent_bodies: &Query<&RigidBodyHandleComponent>,
    physics: &PhysicsWorld,
) -> Option<(Vec3, Quat, Vec3)> {
    let parent_body = parent_body_handle(child_of, parent_parents, parent_bodies);
    if child_of.is_some() && parent_body.is_none() {
        return None;
    }
    let (position, rotation) = parented_world_pose(
        transform,
        child_of,
        parent_transforms,
        parent_parents,
        parent_bodies,
        physics,
    );
    let velocity = parent_body
        .and_then(|handle| physics.rigid_body_set.get(handle))
        .map(|rb| {
            let offset = position - rb_pos(rb);
            rb_vel(rb) + rb_angvel(rb).cross(offset)
        })
        .unwrap_or(Vec3::ZERO);
    Some((position, rotation, velocity))
}
