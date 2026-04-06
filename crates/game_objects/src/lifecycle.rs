use bevy::prelude::*;
use common::GameObjectKind;
use net::message::{NetworkID, NetworkIDResource, SpawnCommand};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};

use crate::level::{SpawnPoint, parented_world_pose};
use crate::SpawnGameObjectCommand;

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
    let count = spawn_points
        .iter()
        .filter(|(_, sp, _, _)| sp.team == team)
        .count();
    if count == 0 {
        return None;
    }
    let Some((_, _, transform, child_of)) = spawn_points
        .iter()
        .filter(|(_, sp, _, _)| sp.team == team)
        .nth(counter % count)
    else {
        return None;
    };
    Some(parented_world_pose(
        transform,
        child_of,
        parent_transforms,
        parent_parents,
        parent_bodies,
        physics,
    ))
}
