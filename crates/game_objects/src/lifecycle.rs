use bevy::prelude::*;
use common::GameObjectKind;
use net::message::{NetworkID, NetworkIDResource, SpawnCommand};

use crate::SpawnGameObjectCommand;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_angvel, rb_pos, rb_vel};

use crate::level::{SpawnPoint, parented_world_pose};

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
        position,
        starting_velocity,
        rotation,
        server_tick,
        kind,
    };
    let entity = commands.spawn_empty().id();
    commands.queue(SpawnGameObjectCommand {
        entity,
        cmd: cmd.clone(),
    });
    (entity, net_id, cmd)
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
    let mut current_parent = child_of.map(ChildOf::parent);
    let mut parent_body = None;
    while let Some(parent) = current_parent {
        if let Ok(handle) = parent_bodies.get(parent) {
            parent_body = physics.rigid_body_set.get(handle.0);
            break;
        }
        current_parent = parent_parents.get(parent).ok().map(ChildOf::parent);
    }
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
        .map(|rb| {
            let offset = position - rb_pos(rb);
            rb_vel(rb) + rb_angvel(rb).cross(offset)
        })
        .unwrap_or(Vec3::ZERO);
    Some((position, rotation, velocity))
}
