use crate::helpers::find_networked_entity;
use crate::resources::*;
use bevy::prelude::*;
use game_objects::pawn::biped::{BipedPawnComponent, WeaponSlots};
use game_objects::pawn::vehicle::*;
use game_objects::pawn::{HeldWeaponMap, PawnInputKind, PlayerRegistry, SeatedInVehicle};
use game_objects::weapon::{WeaponConfig, WeaponState};
use game_objects::*;
use net::message::*;
use net::quic::*;
use physics::physics_world::*;

pub(super) fn handle_input(
    conn_id: ConnectionId,
    input_seq: u64,
    kind: PawnInputKind,
    pending_inputs: &mut PendingInputs,
) {
    let newest_seen = pending_inputs
        .0
        .get(&conn_id)
        .map(|(seq, _)| *seq)
        .unwrap_or(0);
    if input_seq > newest_seen {
        pending_inputs.0.insert(conn_id, (input_seq, kind));
    }
}

pub(super) fn handle_flashlight_toggle(
    conn_id: ConnectionId,
    registry: &PlayerRegistry,
    bipeds: &mut Query<&mut BipedPawnComponent>,
    quic: &mut QuicManager,
) {
    let Some((entity, net_id)) = registry.get_character_by_conn(conn_id) else {
        return;
    };
    let Ok(mut biped) = bipeds.get_mut(entity) else {
        return;
    };
    biped.flashlight_on = !biped.flashlight_on;
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::FlashlightState(net_id.clone(), biped.flashlight_on),
    );
}

pub(super) fn handle_interact(
    conn_id: ConnectionId,
    target_net_id: NetworkID,
    registry: &mut PlayerRegistry,
    pending_inputs: &PendingInputs,
    all_networked: &NetworkEntityMap,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    net_ids: &Query<&NetworkID>,
    vehicles: &Query<&VehicleComponent>,
    driver_seats: &mut Query<(&mut DriverSeat, &Transform)>,
    commands: &mut Commands,
) {
    let Some((player_entity, player_net_id)) = registry.get_by_conn(conn_id) else {
        return;
    };
    let Some(target_entity) = find_networked_entity(all_networked, &target_net_id) else {
        return;
    };
    let player_net_id = player_net_id.clone();
    if try_vehicle_interact(
        conn_id,
        player_entity,
        &player_net_id,
        target_entity,
        &target_net_id,
        registry,
        quic,
        world,
        net_ids,
        vehicles,
        driver_seats,
        commands,
    ) {
        return;
    }
    try_weapon_interact(
        player_entity,
        player_net_id,
        target_entity,
        target_net_id,
        quic,
        world,
        weapon_runtime,
        held_weapons,
        pawn_slots,
        commands,
        pending_inputs
            .0
            .get(&conn_id)
            .map(|(_, input)| input)
            .and_then(|input| biped_aim_dir(world, player_entity, Some(input)))
            .unwrap_or_else(|| body_forward(world, player_entity)),
    );
}

fn body_position(world: &PhysicsWorld, entity: Entity) -> Option<Vec3> {
    world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(rb_pos)
}

fn body_forward(world: &PhysicsWorld, entity: Entity) -> Vec3 {
    world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| rb_rot(rb) * Vec3::NEG_Z)
        .unwrap_or(Vec3::NEG_Z)
}

fn weapon_drop_pose(world: &PhysicsWorld, player_entity: Entity, drop_dir: Vec3) -> (Vec3, Vec3) {
    let pos = body_position(world, player_entity).unwrap_or(Vec3::ZERO);
    let forward = drop_dir
        .normalize_or_zero()
        .try_normalize()
        .unwrap_or_else(|| body_forward(world, player_entity));
    let velocity = world
        .entity_to_handle
        .get(&player_entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(rb_vel)
        .unwrap_or(Vec3::ZERO);
    (pos + forward, velocity + forward * 8.0)
}

fn biped_aim_dir(
    world: &PhysicsWorld,
    entity: Entity,
    input: Option<&PawnInputKind>,
) -> Option<Vec3> {
    let PawnInputKind::Biped(input) = input? else {
        return None;
    };
    world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| {
            rb_rot(rb)
                * Quat::from_rotation_y(input.look_yaw)
                * Quat::from_rotation_x(input.look_pitch)
                * Vec3::NEG_Z
        })
}

fn drop_weapon(
    weapon_id: NetworkID,
    owner_id: NetworkID,
    weapon_entity: Entity,
    owner_entity: Entity,
    drop_dir: Vec3,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    quic: &mut QuicManager,
) {
    held_weapons.0.remove(&weapon_id);
    let depleted = weapon_runtime
        .get_mut(weapon_entity)
        .ok()
        .is_some_and(|(state, _)| game_objects::weapon::is_depleted(&state));
    if depleted {
        commands.entity(weapon_entity).despawn();
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::DespawnCommand(weapon_id),
        );
        return;
    }
    let (drop_pos, drop_velocity) = weapon_drop_pose(world, owner_entity, drop_dir);
    if game_objects::weapon::helpers::drop_or_despawn_weapon(
        commands,
        world,
        weapon_entity,
        None,
        drop_pos,
        drop_velocity,
    ) {
        return;
    }
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::WeaponDrop(weapon_id, owner_id, drop_pos),
    );
}

fn try_vehicle_interact(
    conn_id: ConnectionId,
    player_entity: Entity,
    player_net_id: &NetworkID,
    target_entity: Entity,
    target_net_id: &NetworkID,
    registry: &mut PlayerRegistry,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    net_ids: &Query<&NetworkID>,
    vehicles: &Query<&VehicleComponent>,
    driver_seats: &mut Query<(&mut DriverSeat, &Transform)>,
    commands: &mut Commands,
) -> bool {
    let Ok(vehicle) = vehicles.get(target_entity) else {
        return false;
    };
    let Ok((mut cockpit, seat_transform)) = driver_seats.get_mut(vehicle.driver_seat) else {
        return false;
    };

    if cockpit.occupant.is_some() && player_entity == target_entity {
        let Some(biped_entity) = exit_vehicle(world, target_entity, &mut cockpit, seat_transform)
        else {
            return true;
        };
        let Ok(biped_net_id) = net_ids.get(biped_entity) else {
            return true;
        };
        commands.entity(biped_entity).remove::<SeatedInVehicle>();
        registry.set_controlled(conn_id, biped_entity, biped_net_id.clone());
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::SeatState(biped_net_id.clone(), None),
        );
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::Possess(biped_net_id.clone()),
        );
        return true;
    }

    if cockpit.occupant.is_some()
        || !vehicle_in_range(
            world,
            player_entity,
            target_entity,
            &cockpit,
            seat_transform,
        )
    {
        return true;
    }

    if enter_vehicle(
        world,
        player_entity,
        target_entity,
        &mut cockpit,
        seat_transform,
    ) {
        commands
            .entity(player_entity)
            .insert(SeatedInVehicle(target_entity));
        registry.set_controlled(conn_id, target_entity, target_net_id.clone());
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::SeatState(player_net_id.clone(), Some(target_net_id.clone())),
        );
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::Possess(target_net_id.clone()),
        );
    }
    true
}

fn vehicle_in_range(
    world: &PhysicsWorld,
    player_entity: Entity,
    target_entity: Entity,
    cockpit: &DriverSeat,
    seat_transform: &Transform,
) -> bool {
    let player_pos = body_position(world, player_entity);
    let seat_pos = world
        .entity_to_handle
        .get(&target_entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| seat_world_point(rb_pos(rb), rb_rot(rb), seat_transform.translation));
    matches!((player_pos, seat_pos), (Some(a), Some(b)) if {
        let d = a - b;
        d.x * d.x + d.y * d.y + d.z * d.z
            < (cockpit.interact_radius + 4.0) * (cockpit.interact_radius + 4.0)
    })
}

fn try_weapon_interact(
    player_entity: Entity,
    player_net_id: NetworkID,
    target_entity: Entity,
    target_net_id: NetworkID,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    commands: &mut Commands,
    drop_dir: Vec3,
) {
    if held_weapons.0.contains_key(&target_net_id) {
        return;
    }
    let player_pos = body_position(world, player_entity);
    let weapon_pos = body_position(world, target_entity);
    if !matches!((player_pos, weapon_pos), (Some(pp), Some(wp)) if pp.distance(wp) < 2.0) {
        return;
    }
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    if slots.is_full() {
        let Some((drop_id, drop_entity)) =
            game_objects::weapon::helpers::drop_active_slot(&mut slots)
        else {
            return;
        };
        drop(slots);
        drop_weapon(
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
        let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
            return;
        };
        let _ = game_objects::weapon::helpers::assign_pickup_slot(
            &mut slots,
            target_net_id.clone(),
            target_entity,
        );
        held_weapons.0.insert(target_net_id.clone(), player_entity);
    } else {
        let _ = game_objects::weapon::helpers::assign_pickup_slot(
            &mut slots,
            target_net_id.clone(),
            target_entity,
        );
        held_weapons.0.insert(target_net_id.clone(), player_entity);
    }
    game_objects::weapon::helpers::pickup_world_weapon(world, target_entity);
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::WeaponPickup(target_net_id, player_net_id),
    );
}

pub(super) fn handle_drop_weapon(
    conn_id: ConnectionId,
    registry: &PlayerRegistry,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    held_weapons: &mut HeldWeaponMap,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    commands: &mut Commands,
    quic: &mut QuicManager,
    drop_dir: Vec3,
) {
    let Some((player_entity, player_net_id)) = registry.get_by_conn(conn_id) else {
        return;
    };
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let Some((weapon_id, weapon_entity)) =
        game_objects::weapon::helpers::drop_active_slot(&mut slots)
    else {
        return;
    };
    drop(slots);
    drop_weapon(
        weapon_id,
        player_net_id.clone(),
        weapon_entity,
        player_entity,
        drop_dir,
        world,
        weapon_runtime,
        held_weapons,
        commands,
        quic,
    );
}

pub(super) fn handle_fire_request(
    conn_id: ConnectionId,
    weapon_net_id: NetworkID,
    kind: GameObjectKind,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
    quic: &mut QuicManager,
    tick: u64,
) {
    let Some((shooter_entity, _)) = registry.get_by_conn(conn_id) else {
        return;
    };
    let shooter_holds = pawn_slots
        .get(shooter_entity)
        .map(|s| {
            s.primary.0.as_ref() == Some(&weapon_net_id)
                || s.pocket.0.as_ref() == Some(&weapon_net_id)
        })
        .unwrap_or(false);
    if !shooter_holds {
        return;
    }
    let Some(weapon_entity) = find_networked_entity(all_networked, &weapon_net_id) else {
        return;
    };
    let Ok((mut weapon_state, weapon_config)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    if weapon_config.projectile_kind != kind
        || !weapon::consume_round(&mut weapon_state, weapon_config)
    {
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::WeaponState(weapon_net_id, weapon_state.snapshot()),
        );
        return;
    }
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::WeaponState(weapon_net_id.clone(), weapon_state.snapshot()),
    );
    let Some(fired) = projectile::fire_authoritative(
        kind,
        origin,
        dir,
        shooter_entity,
        tick,
        weapon_entity,
        temp_id,
        commands,
        world,
        net_ids,
    ) else {
        return;
    };
    quic.send(
        SendTarget::AllExcept(conn_id),
        Channel::Unordered,
        &MsgType::SpawnCommand(fired.spawn_cmd),
    );
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::ProjectileConfirm {
            temp_id,
            net_id: fired.net_id,
        },
    );
}

pub(super) fn handle_reload_weapon(
    conn_id: ConnectionId,
    weapon_net_id: NetworkID,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    quic: &mut QuicManager,
) {
    let Some((shooter_entity, _)) = registry.get_by_conn(conn_id) else {
        return;
    };
    let shooter_holds = pawn_slots
        .get(shooter_entity)
        .map(|s| {
            s.primary.0.as_ref() == Some(&weapon_net_id)
                || s.pocket.0.as_ref() == Some(&weapon_net_id)
        })
        .unwrap_or(false);
    if !shooter_holds {
        return;
    }
    let Some(weapon_entity) = find_networked_entity(all_networked, &weapon_net_id) else {
        return;
    };
    let Ok((mut weapon_state, weapon_config)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    let started = weapon::start_reload(&mut weapon_state, weapon_config);
    quic.send(
        if started {
            SendTarget::All
        } else {
            SendTarget::One(conn_id)
        },
        Channel::Ordered,
        &MsgType::WeaponState(weapon_net_id, weapon_state.snapshot()),
    );
}
