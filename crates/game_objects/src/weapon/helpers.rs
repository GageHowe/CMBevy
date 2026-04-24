use bevy::prelude::*;
use common::{GameObjectKind, NetworkID};
use physics::physics_world::*;
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder};

#[cfg(feature = "client")]
use crate::pawn::CameraEffector;
#[cfg(feature = "client")]
use crate::pawn::biped::viewmodel_offset;
use crate::{
    generic::attach_hull_collider,
    pawn::WeaponSlots,
    sound::SoundQueue,
    weapon::{AimReticle, FireCtx, WeaponComponent, WeaponState},
};

pub fn shooter_mass(world: &PhysicsWorld, shooter: Option<Entity>) -> f32 {
    shooter
        .and_then(|e| world.entity_to_handle.get(&e).copied())
        .and_then(|h| world.rigid_body_set.get(h))
        .map(|rb| rb.mass())
        .unwrap_or(0.0)
}

pub fn queue_fire_sound(
    sound: Option<&mut SoundQueue>,
    local_event: &'static str,
) {
    let Some(sound) = sound else {
        return;
    };
    sound.play_2d(local_event);
}

pub fn fire_projectile<F, R>(
    ctx: &mut FireCtx,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    speed: f32,
    spawn: F,
) -> u32
where
    F: FnOnce(Vec3, Vec3, Vec3, &mut Commands, &mut PhysicsWorld, Option<Entity>, u32) -> R,
{
    let velocity =
        crate::projectile::helpers::projectile_velocity(world, ctx.shooter, ctx.aim_dir, speed);
    let temp_id = crate::projectile::helpers::next_temp_id(ctx.id_counter.as_deref_mut());
    let shooter_velocity = crate::projectile::helpers::shooter_velocity(world, ctx.shooter);
    let _ = spawn(ctx.origin, velocity, shooter_velocity, commands, world, ctx.shooter, temp_id);
    #[cfg(feature = "client")]
    send_fire_request(
        ctx.quic.as_deref_mut(),
        ctx.net_id,
        ctx.weapon_config.projectile_kind.clone(),
        temp_id,
        ctx.origin,
        ctx.aim_dir,
    );
    temp_id
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
    quic.send_to_server(
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
    world.apply_game_impulse(shooter, impulse, Some(shooter_net_id), ctx.predicted.as_deref_mut());
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
    world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
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
        Transform { translation: cmd.position, rotation: cmd.rotation, scale: Vec3::ONE },
        cmd.net_id.clone(),
        weapon,
    ));
    #[cfg(feature = "client")]
    {
        let scene = world.resource::<AssetServer>().load(_model_path);
        world.entity_mut(entity).insert((SceneRoot(scene), Visibility::default()));
    }
}

pub fn place_world_weapon(
    world: &mut PhysicsWorld,
    weapon_entity: Entity,
    drop_pos: Vec3,
    drop_velocity: Vec3,
) {
    world.set_body_enabled(weapon_entity, true);
    world.teleport_body(weapon_entity, drop_pos);
    if let Some(&handle) = world.entity_to_handle.get(&weapon_entity)
        && let Some(rb) = world.rigid_body_set.get_mut(handle)
    {
        rb.set_linvel(Vector3::new(drop_velocity.x, drop_velocity.y, drop_velocity.z), true);
        rb.set_angvel(Vector3::ZERO, true);
        rb.wake_up(true);
    }
}

pub fn drop_pose(world: &PhysicsWorld, owner: Entity, drop_dir: Vec3) -> (Vec3, Vec3) {
    let Some(body) =
        world.entity_to_handle.get(&owner).and_then(|&handle| world.rigid_body_set.get(handle))
    else {
        return (Vec3::ZERO, Vec3::ZERO);
    };
    let forward =
        drop_dir.normalize_or_zero().try_normalize().unwrap_or_else(|| rb_rot(body) * Vec3::NEG_Z);
    let velocity = rb_vel(body) + forward * 8.0;
    (rb_pos(body) + forward, velocity)
}

pub fn predicted_drop_pos(drop_pos: Vec3, drop_velocity: Vec3, rtt_secs: f32) -> Vec3 {
    drop_pos + drop_velocity * (rtt_secs * 0.5)
}

pub fn body_velocity(world: &PhysicsWorld, entity: Entity) -> Vec3 {
    world
        .entity_to_handle
        .get(&entity)
        .and_then(|&handle| world.rigid_body_set.get(handle))
        .map(rb_vel)
        .unwrap_or(Vec3::ZERO)
}

pub fn restore_world_weapon(
    world: &mut World,
    weapon_entity: Entity,
    drop_pos: Vec3,
    drop_velocity: Vec3,
) {
    let handle = world.resource::<PhysicsWorld>().entity_to_handle.get(&weapon_entity).copied();
    #[cfg(feature = "client")]
    {
        let mut entity = world.entity_mut(weapon_entity);
        entity.remove_parent_in_place();
    }
    {
        let mut entity = world.entity_mut(weapon_entity);
        entity.insert((crate::interaction::Interactable { range: 2.0 }, Visibility::Inherited));
        if let Some(handle) = handle {
            entity.insert(RigidBodyHandleComponent(handle));
        }
    }
    world.resource_scope(|_, mut physics: Mut<PhysicsWorld>| {
        place_world_weapon(&mut physics, weapon_entity, drop_pos, drop_velocity);
    });
}

pub fn drop_or_despawn_weapon(
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    weapon_entity: Entity,
    weapon_state: Option<Mut<WeaponState>>,
    drop_pos: Vec3,
    drop_velocity: Vec3,
) -> bool {
    if let Some(mut weapon_state) = weapon_state {
        crate::weapon::cancel_reload(&mut weapon_state);
        if crate::weapon::is_depleted(&weapon_state) {
            commands.entity(weapon_entity).despawn();
            return true;
        }
    }
    #[cfg(feature = "client")]
    detach_viewmodel(commands, world, weapon_entity);
    place_world_weapon(world, weapon_entity, drop_pos, drop_velocity);
    false
}

pub fn clear_inactive_slot_reload(weapon_state: Option<Mut<WeaponState>>) {
    if let Some(mut weapon_state) = weapon_state {
        crate::weapon::cancel_reload(&mut weapon_state);
    }
}

pub fn pickup_world_weapon(world: &mut PhysicsWorld, weapon_entity: Entity) {
    world.set_body_enabled(weapon_entity, false);
}

pub fn give_world_weapon(
    world: &mut PhysicsWorld,
    slots: &mut WeaponSlots,
    weapon_entity: Entity,
    weapon_id: NetworkID,
) -> bool {
    if slots.assign_pickup(weapon_id, weapon_entity).is_none() {
        return false;
    }
    pickup_world_weapon(world, weapon_entity);
    true
}

#[cfg(feature = "client")]
fn attach_viewmodel(
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
pub fn attach_local_viewmodel(
    commands: &mut Commands,
    weapon_entity: Entity,
    parent: Entity,
    is_primary: bool,
) {
    attach_viewmodel(commands, weapon_entity, parent, is_primary);
}

#[cfg(feature = "client")]
pub fn attach_remote_viewmodel(commands: &mut Commands, weapon_entity: Entity, parent: Entity) {
    attach_viewmodel(commands, weapon_entity, parent, true);
}

#[cfg(feature = "client")]
pub fn detach_viewmodel(commands: &mut Commands, world: &PhysicsWorld, weapon_entity: Entity) {
    let handle = world.entity_to_handle.get(&weapon_entity).copied();
    commands
        .entity(weapon_entity)
        .remove_parent_in_place()
        .insert((crate::interaction::Interactable { range: 2.0 }, Visibility::Inherited));
    if let Some(handle) = handle {
        commands.entity(weapon_entity).insert(RigidBodyHandleComponent(handle));
    }
}

#[cfg(feature = "client")]
pub fn set_local_slot_visibility(commands: &mut Commands, slots: &WeaponSlots) {
    for (idx, (_, weapon)) in slots.slots.iter().enumerate() {
        if let Some(weapon) = weapon {
            commands.entity(*weapon).insert(if idx == slots.active_index {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}

#[cfg(feature = "client")]
pub fn sync_local_active_weapon(
    commands: &mut Commands,
    slots: &WeaponSlots,
    camera: &mut Query<&mut CameraEffector, With<Camera3d>>,
) {
    set_local_slot_visibility(commands, slots);
    if let Ok(mut camera) = camera.single_mut() {
        camera.reset_zoom();
    }
}
