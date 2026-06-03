use bevy::prelude::*;
use common::NetworkID;
use physics::physics_world::*;
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder};

#[cfg(feature = "client")]
use crate::pawn::CameraEffector;
#[cfg(feature = "client")]
use crate::pawn::biped::BipedPawnComponent;
#[cfg(feature = "client")]
use crate::pawn::biped::viewmodel_offset;
use crate::{
    generic::attach_hull_collider,
    interaction::InteractionName,
    pawn::WeaponSlots,
    reticle::AimReticle,
    sound::SoundQueue,
    weapon::{FireCtx, WeaponComponent, WeaponState},
};

pub fn shooter_mass(world: &PhysicsWorld, shooter: Option<Entity>) -> f32 {
    shooter
        .and_then(|e| world.entity_to_handle.get(&e).copied())
        .and_then(|h| world.rigid_body_set.get(h))
        .map(|rb| rb.mass())
        .unwrap_or(0.0)
}

pub fn queue_fire_sound(sound: Option<&mut SoundQueue>, local_event: &'static str) {
    let Some(sound) = sound else {
        return;
    };
    sound.play_2d(local_event);
}

pub fn apply_spread(aim_dir: Vec3, spread_radians: f32) -> Vec3 {
    let aim_dir = aim_dir.normalize_or_zero();
    if spread_radians <= 0.0 || aim_dir == Vec3::ZERO {
        return aim_dir;
    }
    let right = aim_dir.any_orthonormal_vector();
    let up = aim_dir.cross(right).normalize_or_zero();
    let yaw = fastrand::f32() * 2.0 * spread_radians - spread_radians;
    let pitch = fastrand::f32() * 2.0 * spread_radians - spread_radians;
    (aim_dir + right * yaw.tan() + up * pitch.tan()).normalize_or_zero()
}

pub fn fire_projectile(
    ctx: &mut FireCtx,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
) -> u32 {
    let temp_id = crate::projectile::next_temp_id(ctx.id_counter.as_deref_mut());
    fire_projectile_with_dir(ctx, world, commands, temp_id, ctx.aim_dir);
    temp_id
}

pub fn fire_projectile_with_dir(
    ctx: &mut FireCtx,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    temp_id: u32,
    dir: Vec3,
) {
    let Some(projectile) = ctx.weapon_config.projectile else {
        return;
    };
    let speed = ctx.weapon_config.prediction_projectile_speed.unwrap_or(0.0);
    let velocity = crate::projectile::projectile_velocity(world, ctx.shooter, dir, speed);
    let shooter_velocity = crate::projectile::shooter_velocity(world, ctx.shooter);
    #[cfg(feature = "client")]
    let projectile_entity = crate::projectile::spawn(
        projectile,
        ctx.weapon_config.projectile_gravity_scale,
        ctx.origin,
        velocity,
        shooter_velocity,
        commands,
        world,
        ctx.shooter,
        Some(temp_id),
    );
    #[cfg(feature = "client")]
    if let Some(decorate_projectile) = ctx.weapon_config.decorate_projectile {
        commands.queue(move |world: &mut World| {
            decorate_projectile(projectile_entity, world);
        });
    }
    #[cfg(not(feature = "client"))]
    crate::projectile::spawn(
        projectile,
        ctx.weapon_config.projectile_gravity_scale,
        ctx.origin,
        velocity,
        shooter_velocity,
        commands,
        world,
        ctx.shooter,
        Some(temp_id),
    );
    #[cfg(feature = "client")]
    send_fire_request(
        ctx.quic.as_deref_mut(),
        ctx.net_id,
        temp_id,
        ctx.origin,
        dir,
    );
}

#[cfg(feature = "client")]
pub fn send_fire_request(
    quic: Option<&mut net::quic::QuicManager>,
    weapon_net_id: Option<&NetworkID>,
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
    let position = cmd.position_or_zero();
    let rotation = cmd.rotation_or_identity();
    let velocity = cmd.velocity_or_zero();
    let angular_velocity = cmd.angular_velocity_or_zero();
    let rb_handle = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let rb = RigidBodyBuilder::dynamic()
            .translation(position)
            .linvel(Vector3::new(velocity.x, velocity.y, velocity.z))
            .angular_damping(0.3)
            .build();
        let rb_handle = physics.insert_body(entity, rb);
        if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
            rb.set_rotation(rotation, true);
            rb.set_angvel(
                Vector3::new(angular_velocity.x, angular_velocity.y, angular_velocity.z),
                true,
            );
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
    spawn_name: &'static str,
    world: &mut World,
    display_name: &'static str,
    _model_path: &'static str,
    crosshair_path: &'static str,
    prediction_projectile_speed: Option<f32>,
    weapon: impl Bundle,
) {
    let position = cmd.position_or_zero();
    let rotation = cmd.rotation_or_identity();
    world.entity_mut(entity).insert((
        crate::SpawnReplicated(spawn_name),
        WeaponComponent,
        AimReticle(crosshair_path, prediction_projectile_speed),
        crate::interaction::Interactable { range: 2.0 },
        InteractionName(display_name),
        Transform {
            translation: position,
            rotation,
            scale: Vec3::ONE,
        },
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
        rb.set_linvel(
            Vector3::new(drop_velocity.x, drop_velocity.y, drop_velocity.z),
            true,
        );
        rb.set_angvel(Vector3::ZERO, true);
        rb.wake_up(true);
    }
}

pub fn drop_pose(world: &PhysicsWorld, owner: Entity, drop_dir: Vec3) -> (Vec3, Vec3) {
    let Some(body) = world
        .entity_to_handle
        .get(&owner)
        .and_then(|&handle| world.rigid_body_set.get(handle))
    else {
        return (Vec3::ZERO, Vec3::ZERO);
    };
    let forward = drop_dir
        .normalize_or_zero()
        .try_normalize()
        .unwrap_or_else(|| rb_rot(body) * Vec3::NEG_Z);
    let velocity = rb_vel(body) + forward * 8.0;
    (rb_pos(body) + forward, velocity)
}

pub fn restore_world_weapon(
    world: &mut World,
    weapon_entity: Entity,
    drop_pos: Vec3,
    drop_velocity: Vec3,
) {
    #[cfg(feature = "client")]
    {
        let mut entity = world.entity_mut(weapon_entity);
        entity.remove_parent_in_place();
    }
    world.entity_mut(weapon_entity).insert((
        crate::interaction::Interactable { range: 2.0 },
        Visibility::Inherited,
    ));
    let mut physics = world.resource_mut::<PhysicsWorld>();
    place_world_weapon(&mut physics, weapon_entity, drop_pos, drop_velocity);
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
    detach_viewmodel(commands, weapon_entity);
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

pub fn interact_pickup(
    player_entity: Entity,
    player_net_id: NetworkID,
    target_entity: Entity,
    target_net_id: NetworkID,
    quic: &mut net::quic::QuicManager,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &crate::weapon::WeaponConfig)>,
    held_weapons: &mut crate::pawn::HeldWeaponMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    commands: &mut Commands,
    interactables: &Query<&crate::interaction::Interactable>,
    drop_dir: Vec3,
) {
    if held_weapons.0.contains_key(&target_net_id) {
        return;
    }
    let Ok(interactable) = interactables.get(target_entity) else {
        return;
    };
    if !world.entities_within_range(player_entity, target_entity, interactable.range) {
        return;
    }
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let dropped = if slots.is_full() {
        slots.remove_active()
    } else {
        None
    };
    drop(slots);
    if let Some((drop_id, drop_entity)) = dropped {
        drop_from_owner(
            drop_id,
            player_net_id.clone(),
            drop_entity,
            player_entity,
            drop_dir,
            world,
            weapon_runtime,
            held_weapons,
            commands,
            quic,
        );
    }
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let _ = slots.assign_pickup(target_net_id.clone(), target_entity);
    held_weapons.0.insert(target_net_id.clone(), player_entity);
    pickup_world_weapon(world, target_entity);
    quic.send(
        net::quic::SendTarget::All,
        net::quic::Channel::Ordered,
        &net::message::MsgType::WeaponPickup(target_net_id, player_net_id),
    );
}

pub fn drop_from_owner(
    weapon_id: NetworkID,
    owner_id: NetworkID,
    weapon_entity: Entity,
    owner_entity: Entity,
    drop_dir: Vec3,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &crate::weapon::WeaponConfig)>,
    held_weapons: &mut crate::pawn::HeldWeaponMap,
    commands: &mut Commands,
    quic: &mut net::quic::QuicManager,
) {
    held_weapons.0.remove(&weapon_id);
    let (drop_pos, drop_velocity) = drop_pose(world, owner_entity, drop_dir);
    let despawned = {
        let weapon_state = weapon_runtime
            .get_mut(weapon_entity)
            .ok()
            .map(|(state, _)| state);
        drop_or_despawn_weapon(
            commands,
            world,
            weapon_entity,
            weapon_state,
            drop_pos,
            drop_velocity,
        )
    };
    if despawned {
        return;
    }
    quic.send(
        net::quic::SendTarget::All,
        net::quic::Channel::Ordered,
        &net::message::MsgType::WeaponDrop(weapon_id, owner_id, drop_pos),
    );
}

#[cfg(feature = "client")]
pub fn drop_local_active_weapon(
    slots: &mut WeaponSlots,
    drop_pos: Vec3,
    drop_velocity: Vec3,
    weapon_states: &mut Query<&mut WeaponState>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    camera_fx: &mut Query<&mut CameraEffector, With<Camera3d>>,
) -> bool {
    let Some((_weapon_id, weapon_entity)) = slots.remove_active() else {
        return false;
    };
    drop_or_despawn_weapon(
        commands,
        world,
        weapon_entity,
        weapon_states.get_mut(weapon_entity).ok(),
        drop_pos,
        drop_velocity,
    );
    sync_local_active_weapon(commands, slots, camera_fx);
    true
}

#[cfg(feature = "client")]
pub fn pickup_local_world_weapon(
    player_entity: Entity,
    weapon_entity: Entity,
    weapon_id: &NetworkID,
    pitch_parent: Entity,
    slots: &mut WeaponSlots,
    drop_pos: Vec3,
    drop_velocity: Vec3,
    weapon_states: &mut Query<&mut WeaponState>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    camera_fx: &mut Query<&mut CameraEffector, With<Camera3d>>,
    interaction_names: &Query<&InteractionName>,
) -> bool {
    let _ = player_entity;
    if slots.is_full() {
        drop_local_active_weapon(
            slots,
            drop_pos,
            drop_velocity,
            weapon_states,
            commands,
            world,
            camera_fx,
        );
    }
    let Some((is_primary, _)) = slots.assign_pickup(weapon_id.clone(), weapon_entity) else {
        return false;
    };
    pickup_world_weapon(world, weapon_entity);
    attach_viewmodel(commands, weapon_entity, pitch_parent, is_primary);
    sync_local_active_weapon(commands, slots, camera_fx);
    if let Ok(name) = interaction_names.get(weapon_entity) {
        crate::messages::push(commands, format!("Picked up {}", name.0));
    }
    true
}

#[cfg(feature = "client")]
pub fn attach_viewmodel(
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
pub fn detach_viewmodel(commands: &mut Commands, weapon_entity: Entity) {
    commands
        .entity(weapon_entity)
        .remove_parent_in_place()
        .insert((
            crate::interaction::Interactable { range: 2.0 },
            Visibility::Inherited,
        ));
}

#[cfg(feature = "client")]
pub fn set_local_slot_visibility(commands: &mut Commands, slots: &WeaponSlots) {
    for (idx, (_, weapon)) in slots.slots.iter().enumerate() {
        if let Some(weapon) = weapon {
            commands
                .entity(*weapon)
                .insert(if idx == slots.active_index {
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

#[cfg(feature = "client")]
pub fn apply_pickup_message(
    weapon_id: &NetworkID,
    carrier_net_id: &NetworkID,
    local_net_id: Option<&NetworkID>,
    networked: &crate::NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut bevy::ecs::system::ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<crate::pawn::Possessed>>,
        Query<&BipedPawnComponent>,
        Query<&mut BipedPawnComponent>,
    )>,
    interaction_names: &Query<&InteractionName>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> bool {
    let Some(weapon_entity) = networked.get_entity(weapon_id) else {
        return false;
    };
    pickup_world_weapon(world, weapon_entity);
    if local_net_id == Some(carrier_net_id) {
        let (slot_result, pivot_e) = if let Ok((mut slots, biped)) = biped_q.p0().single_mut() {
            (
                slots.assign_pickup(weapon_id.clone(), weapon_entity),
                biped.pitch_pivot,
            )
        } else {
            (None, None)
        };
        if let Some((is_primary, prev_to_hide)) = slot_result {
            if let Some(prev) = prev_to_hide {
                commands.entity(prev).insert(Visibility::Hidden);
            }
            if let Some(parent) = camera.single().ok().or(pivot_e) {
                attach_viewmodel(commands, weapon_entity, parent, is_primary);
            }
            if let Ok(name) = interaction_names.get(weapon_entity) {
                crate::messages::push(commands, format!("Picked up {}", name.0));
            }
        }
        return true;
    }
    let Some(carrier) = networked.get_entity(carrier_net_id) else {
        return false;
    };
    let Some(parent) = ({
        let q = biped_q.p1();
        q.get(carrier).ok().and_then(|b| b.pitch_pivot)
    }) else {
        return false;
    };
    attach_viewmodel(commands, weapon_entity, parent, true);
    true
}

#[cfg(feature = "client")]
pub fn apply_drop_message(
    weapon_id: &NetworkID,
    carrier_id: &NetworkID,
    drop_pos: Vec3,
    rtt_secs: f32,
    local_net_id: Option<&NetworkID>,
    networked: &crate::NetworkEntityMap,
    biped_q: &mut bevy::ecs::system::ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<crate::pawn::Possessed>>,
        Query<&BipedPawnComponent>,
        Query<&mut BipedPawnComponent>,
    )>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Some(weapon_entity) = networked.get_entity(weapon_id) else {
        return;
    };
    let Some(carrier_entity) = networked.get_entity(carrier_id) else {
        return;
    };
    let drop_velocity = world.body(carrier_entity).map(rb_vel).unwrap_or(Vec3::ZERO);
    let is_local = local_net_id == Some(carrier_id);
    let drop_pos = if !is_local {
        drop_pos + drop_velocity * (rtt_secs * 0.5)
    } else {
        world.body_pos(carrier_entity).unwrap_or(drop_pos) + drop_velocity.normalize_or_zero()
    };
    place_world_weapon(world, weapon_entity, drop_pos, drop_velocity);
    if is_local && let Ok((mut slots, _)) = biped_q.p0().single_mut() {
        slots.remove_by_net_id(weapon_id);
        set_local_slot_visibility(commands, &slots);
    }
    detach_viewmodel(commands, weapon_entity);
}
