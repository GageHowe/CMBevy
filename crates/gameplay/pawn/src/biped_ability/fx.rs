use bevy::prelude::*;
use crate::net::{
    message::{AbilityFx, NetworkID},
    quic::Channel,
};
#[cfg(feature = "client")]
use particles_plugin::prelude::{EffectSpawner, spawn_dash_effect, spawn_jetpack_effect};
#[cfg(feature = "client")]
use physics::physics_world::PhysicsWorld;
#[cfg(feature = "client")]
use physics::physics_world::{rb_pos, rb_rot};

#[cfg(feature = "client")]
use crate::{NetworkEntityMap, pawn::biped::BipedPawnComponent};

#[cfg(feature = "client")]
const JETPACK_OFFSET: Vec3 = Vec3::new(0.0, -0.7, 0.0);

#[cfg(feature = "client")]
#[derive(Component)]
pub(crate) struct JetpackFxTag;

#[cfg(feature = "client")]
#[derive(Component, Clone, Copy)]
pub(crate) struct JetpackFxOwner(Entity);

pub fn fx_channel(fx: AbilityFx) -> Channel {
    match fx {
        AbilityFx::Jetpack(_) => Channel::Ordered,
        AbilityFx::Dash(_) => Channel::Unreliable,
    }
}

#[cfg(feature = "client")]
pub fn queue_fx(owner: Entity, fx: AbilityFx, world: &PhysicsWorld, commands: &mut Commands) {
    match fx {
        AbilityFx::Jetpack(active) => queue_jetpack_fx(owner, active, commands),
        AbilityFx::Dash(dir) => queue_dash_fx(owner, dir, world, commands),
    }
}

#[cfg(feature = "client")]
pub fn queue_remote_fx(
    net_id: &NetworkID,
    fx: AbilityFx,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    world: &PhysicsWorld,
    commands: &mut Commands,
) {
    if local_net_id == Some(net_id) {
        return;
    }
    let Some(entity) = networked.get_entity(net_id) else {
        return;
    };
    queue_fx(entity, fx, world, commands);
}

#[cfg(feature = "client")]
pub fn queue_jetpack_fx(owner: Entity, active: bool, commands: &mut Commands) {
    commands.queue(move |world: &mut World| {
        let current = world
            .get::<BipedPawnComponent>(owner)
            .and_then(|biped| biped.jetpack_fx_entity);
        if active {
            if let Some(fx_entity) = current {
                if let Some(mut spawner) = world.get_mut::<EffectSpawner>(fx_entity) {
                    spawner.active = true;
                }
                return;
            }
            let fx_entity = spawn_jetpack_effect(world);
            world
                .entity_mut(fx_entity)
                .insert((JetpackFxTag, JetpackFxOwner(owner)));
            if let Some(mut biped) = world.get_mut::<BipedPawnComponent>(owner) {
                biped.jetpack_fx_entity = Some(fx_entity);
            }
            return;
        }
        if let Some(fx_entity) = current
            && let Some(mut spawner) = world.get_mut::<EffectSpawner>(fx_entity)
        {
            spawner.active = false;
        }
    });
}

#[cfg(feature = "client")]
pub fn queue_dash_fx(owner: Entity, dir: Vec3, world: &PhysicsWorld, commands: &mut Commands) {
    let Some(&handle) = world.entity_to_handle.get(&owner) else {
        return;
    };
    let Some(rb) = world.rigid_body_set.get(handle) else {
        return;
    };
    let world_emit_dir = if dir.length_squared() > 1e-6 {
        -dir.normalize()
    } else {
        Vec3::NEG_Y
    };
    let local_emit_dir = { rb_rot(rb).inverse() * world_emit_dir };
    commands.queue(move |world: &mut World| {
        spawn_dash_effect(world, owner, local_emit_dir);
    });
}

#[cfg(feature = "client")]
pub(crate) fn sync_jetpack_fx_velocity(
    world: Res<PhysicsWorld>,
    bipeds: Query<(Entity, &BipedPawnComponent)>,
    mut jetpack_fx: Query<&mut Transform, With<JetpackFxTag>>,
) {
    for (owner, biped) in &bipeds {
        let Some(fx_entity) = biped.jetpack_fx_entity else {
            continue;
        };
        let Ok(mut transform) = jetpack_fx.get_mut(fx_entity) else {
            continue;
        };
        let Some(&handle) = world.entity_to_handle.get(&owner) else {
            continue;
        };
        let Some(rb) = world.rigid_body_set.get(handle) else {
            continue;
        };
        let rot = rb_rot(rb);
        transform.translation = rb_pos(rb) + rot * JETPACK_OFFSET;
        transform.rotation = rot;
    }
}

#[cfg(feature = "client")]
pub(crate) fn cleanup_orphaned_jetpack_fx(
    mut commands: Commands,
    bipeds: Query<&BipedPawnComponent>,
    jetpack_fx: Query<(Entity, &JetpackFxOwner), With<JetpackFxTag>>,
) {
    for (fx_entity, owner) in &jetpack_fx {
        let keep = bipeds
            .get(owner.0)
            .ok()
            .is_some_and(|biped| biped.jetpack_fx_entity == Some(fx_entity));
        if !keep {
            commands.entity(fx_entity).despawn();
        }
    }
}
