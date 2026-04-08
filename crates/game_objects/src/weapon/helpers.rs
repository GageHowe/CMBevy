use bevy::prelude::*;
use common::GameObjectKind;
use common::NetworkID;
use physics::physics_world::*;

use crate::generic::attach_hull_collider;
use crate::pawn::biped::WeaponSlots;
#[cfg(feature = "client")]
use crate::pawn::biped::viewmodel_offset;
use crate::sound::{SoundQueue, entity_velocity};
#[cfg(feature = "client")]
use crate::weapon::FireCtx;
use crate::weapon::{AimReticle, WeaponComponent};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder};

pub fn shooter_mass(world: &PhysicsWorld, shooter: Option<Entity>) -> f32 {
    shooter
        .and_then(|e| world.entity_to_handle.get(&e).copied())
        .and_then(|h| world.rigid_body_set.get(h))
        .map(|rb| rb.mass())
        .unwrap_or(0.0)
}

pub fn queue_fire_sound(
    sound: Option<&mut SoundQueue>,
    world: &PhysicsWorld,
    shooter: Option<Entity>,
    local: bool,
    local_event: &'static str,
    remote_event: &'static str,
    origin: Vec3,
) {
    let Some(sound) = sound else {
        return;
    };
    if local {
        sound.play_2d(local_event);
    } else {
        sound.play_3d(remote_event, origin, entity_velocity(world, shooter));
    }
}

#[cfg(feature = "client")]
pub fn send_fire_request(
    quic: Option<&mut net::quic::QuicManager>,
    weapon_net_id: Option<&NetworkID>,
    kind: GameObjectKind,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
) {
    let (Some(quic), Some(weapon_net_id)) = (quic, weapon_net_id) else {
        return;
    };
    quic.send(
        net::quic::SendTarget::All,
        net::quic::Channel::Unordered,
        &net::message::MsgType::FireRequest {
            weapon: weapon_net_id.clone(),
            kind,
            temp_id,
            origin,
            dir,
        },
    );
}

#[cfg(feature = "client")]
pub fn apply_local_predicted_impulse(ctx: &mut FireCtx, world: &mut PhysicsWorld, impulse: Vec3) {
    let (Some(shooter), Some(shooter_net_id)) = (ctx.shooter, ctx.shooter_net_id) else {
        return;
    };
    world.apply_game_impulse(
        shooter,
        impulse,
        Some(shooter_net_id),
        ctx.predicted.as_deref_mut(),
    );
}

pub fn make_generic_weapon_physics(
    entity: Entity,
    cmd: &net::message::SpawnCommand,
    hull_path: &'static str,
    collider: ColliderBuilder,
    world: &mut World,
) -> RigidBodyHandle {
    let rb_handle = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let rb = RigidBodyBuilder::dynamic()
            .translation(cmd.position)
            .linvel(Vector3::new(
                cmd.starting_velocity.x,
                cmd.starting_velocity.y,
                cmd.starting_velocity.z,
            ))
            .angular_damping(0.3)
            .build();
        let rb_handle = physics.insert_body(entity, rb);
        if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
            rb.set_rotation(cmd.rotation, true);
        }
        rb_handle
    };
    world
        .entity_mut(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    attach_hull_collider(entity, rb_handle, hull_path, 1.0, collider, world);
    rb_handle
}

pub fn insert_generic_weapon(
    entity: Entity,
    cmd: &net::message::SpawnCommand,
    world: &mut World,
    kind: GameObjectKind,
    _model_path: &'static str,
    crosshair_path: &'static str,
    prediction_projectile_speed: Option<f32>,
    weapon: impl Bundle,
) {
    world.entity_mut(entity).insert((
        WeaponComponent,
        AimReticle(crosshair_path, prediction_projectile_speed),
        kind,
        crate::interaction::Interactable { range: 2.0 },
        Transform {
            translation: cmd.position,
            rotation: cmd.rotation,
            scale: Vec3::ONE,
        },
        cmd.net_id.clone(),
        weapon,
    ));
    #[cfg(feature = "client")]
    {
        let scene = world.resource::<AssetServer>().load(_model_path);
        world
            .entity_mut(entity)
            .insert((SceneRoot(scene), Visibility::default()));
    }
}

pub fn assign_pickup_slot(
    slots: &mut WeaponSlots,
    weapon_id: NetworkID,
    weapon_entity: Entity,
) -> Option<(bool, Option<Entity>)> {
    if slots.primary.0.is_none() {
        slots.primary = (Some(weapon_id), Some(weapon_entity));
        Some((true, None))
    } else if slots.pocket.0.is_none() {
        let prev = slots.active().1;
        slots.pocket = (Some(weapon_id), Some(weapon_entity));
        slots.active_primary = false;
        Some((false, prev))
    } else {
        None
    }
}

pub fn drop_active_slot(slots: &mut WeaponSlots) -> Option<(NetworkID, Entity)> {
    let active = slots.active_mut();
    Some((active.0.take()?, active.1.take()?))
}

pub fn place_world_weapon(
    world: &mut PhysicsWorld,
    weapon_entity: Entity,
    drop_pos: Vec3,
    drop_velocity: Vec3,
) {
    world.teleport_body(weapon_entity, drop_pos);
    if let Some(&handle) = world.entity_to_handle.get(&weapon_entity)
        && let Some(rb) = world.rigid_body_set.get_mut(handle)
    {
        rb.set_linvel(
            Vector3::new(drop_velocity.x, drop_velocity.y, drop_velocity.z),
            true,
        );
        rb.set_angvel(Vector3::ZERO, true);
    }
    world.set_body_enabled(weapon_entity, true);
}

pub fn pickup_world_weapon(world: &mut PhysicsWorld, weapon_entity: Entity) {
    world.set_body_enabled(weapon_entity, false);
}

#[cfg(feature = "client")]
pub fn attach_local_viewmodel(
    commands: &mut Commands,
    weapon_entity: Entity,
    parent: Entity,
    is_primary: bool,
) {
    commands
        .entity(weapon_entity)
        .remove::<crate::interaction::Interactable>()
        .set_parent_in_place(parent)
        .insert(viewmodel_offset(is_primary))
        .insert(Visibility::Inherited);
}

#[cfg(feature = "client")]
pub fn attach_remote_viewmodel(commands: &mut Commands, weapon_entity: Entity, parent: Entity) {
    commands
        .entity(weapon_entity)
        .remove::<crate::interaction::Interactable>()
        .set_parent_in_place(parent)
        .insert(viewmodel_offset(true));
}

#[cfg(feature = "client")]
pub fn detach_viewmodel(commands: &mut Commands, world: &PhysicsWorld, weapon_entity: Entity) {
    let handle = world.entity_to_handle.get(&weapon_entity).copied();
    commands
        .entity(weapon_entity)
        .remove_parent_in_place()
        .insert((
            crate::interaction::Interactable { range: 2.0 },
            Visibility::Inherited,
        ));
    if let Some(handle) = handle {
        commands
            .entity(weapon_entity)
            .insert(RigidBodyHandleComponent(handle));
    }
}
