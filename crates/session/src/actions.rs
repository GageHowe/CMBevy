use bevy::prelude::*;
use game_objects::{
    pawn::{
        HeldWeaponMap, PawnInputKind, PlayerRegistry, SeatedInVehicle, WeaponSlots,
        biped::BipedPawnComponent,
        biped_ability::{DropActiveAbility, OnPickup},
        vehicle::*,
    },
    weapon::{WeaponConfig, WeaponState},
    *,
};
use net::{message::*, quic::*};
use physics::physics_world::*;

use crate::{helpers::find_networked_entity, resources::*};

pub(super) fn handle_input(
    conn_id: ConnectionId,
    input_seq: u64,
    kind: PawnInputKind,
    pending_inputs: &mut PendingInputs,
) {
    let newest_seen = pending_inputs.0.get(&conn_id).map(|(seq, _)| *seq).unwrap_or(0);
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
    let Some((entity, net_id)) = registry.character(conn_id) else {
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
    interactables: &Query<&game_objects::interaction::Interactable>,
    driver_seats: &mut Query<(&mut DriverSeat, &Transform)>,
    commands: &mut Commands,
    on_pickup_q: &Query<&OnPickup>,
) {
    let Some((controlled, _)) = registry.controlled_pawn(conn_id) else {
        return;
    };
    let Some((character, character_net_id)) = registry.character(conn_id) else {
        return;
    };
    let character_net_id = character_net_id.clone();
    let Some(target) = find_networked_entity(all_networked, &target_net_id) else {
        return;
    };
    let aim_dir = pending_inputs
        .0
        .get(&conn_id)
        .map(|(_, input)| input)
        .and_then(|input| biped_aim_dir(world, character, Some(input)))
        .unwrap_or_else(|| body_forward(world, character));

    if vehicles.contains(target) {
        handle_vehicle_interact(
            conn_id,
            controlled,
            character,
            &character_net_id,
            target,
            &target_net_id,
            registry,
            quic,
            world,
            net_ids,
            vehicles,
            driver_seats,
            commands,
        );
        return;
    }

    if handle_ability_pickup_interact(
        conn_id,
        character,
        character_net_id.clone(),
        target,
        target_net_id.clone(),
        world,
        interactables,
        on_pickup_q,
        commands,
        quic,
        aim_dir,
    ) {
        return;
    }

    try_weapon_interact(
        character,
        character_net_id,
        target,
        target_net_id,
        quic,
        world,
        weapon_runtime,
        held_weapons,
        pawn_slots,
        commands,
        interactables,
        aim_dir,
    );
}

fn body_forward(world: &PhysicsWorld, entity: Entity) -> Vec3 {
    world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| rb_rot(rb) * Vec3::NEG_Z)
        .unwrap_or(Vec3::NEG_Z)
}

fn body_position(world: &PhysicsWorld, entity: Entity) -> Option<Vec3> {
    world.entity_to_handle.get(&entity).and_then(|&h| world.rigid_body_set.get(h)).map(rb_pos)
}

fn biped_aim_dir(
    world: &PhysicsWorld,
    entity: Entity,
    input: Option<&PawnInputKind>,
) -> Option<Vec3> {
    let PawnInputKind::Biped(input) = input? else {
        return None;
    };
    world.entity_to_handle.get(&entity).and_then(|&h| world.rigid_body_set.get(h)).map(|rb| {
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
    let (drop_pos, drop_velocity) =
        game_objects::weapon::helpers::drop_pose(world, owner_entity, drop_dir);
    let despawned = {
        let weapon_state = weapon_runtime.get_mut(weapon_entity).ok().map(|(state, _)| state);
        game_objects::weapon::helpers::drop_or_despawn_weapon(
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
        SendTarget::All,
        Channel::Ordered,
        &MsgType::WeaponDrop(weapon_id, owner_id, drop_pos),
    );
}

pub(super) fn handle_set_active_weapon_slot(
    conn_id: ConnectionId,
    active_primary: bool,
    registry: &PlayerRegistry,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    quic: &mut QuicManager,
) {
    let Some((player_entity, _)) = registry.controlled_pawn(conn_id) else {
        return;
    };
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let old_active = slots.active().0.clone();
    let old_active_entity = slots.active().1;
    slots.set_active_primary(active_primary);
    let new_active = slots.active().0.clone();
    if old_active == new_active {
        return;
    }
    let Some(old_weapon_entity) = old_active_entity else {
        return;
    };
    let Some(old_weapon_id) = old_active else {
        return;
    };
    let Ok((mut weapon_state, _)) = weapon_runtime.get_mut(old_weapon_entity) else {
        return;
    };
    game_objects::weapon::cancel_reload(&mut weapon_state);
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::WeaponState(old_weapon_id, *weapon_state),
    );
}

fn handle_vehicle_interact(
    conn_id: ConnectionId,
    controlled: Entity,
    character: Entity,
    character_net_id: &NetworkID,
    target: Entity,
    target_net_id: &NetworkID,
    registry: &mut PlayerRegistry,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    net_ids: &Query<&NetworkID>,
    vehicles: &Query<&VehicleComponent>,
    driver_seats: &mut Query<(&mut DriverSeat, &Transform)>,
    commands: &mut Commands,
) {
    let Ok(vehicle) = vehicles.get(target) else {
        return;
    };
    let Ok((mut cockpit, seat_transform)) = driver_seats.get_mut(vehicle.driver_seat) else {
        return;
    };

    if cockpit.occupant.is_some() && controlled == target {
        let Some(biped_entity) = exit_vehicle(world, target, &mut cockpit, seat_transform) else {
            return;
        };
        let Ok(biped_net_id) = net_ids.get(biped_entity) else {
            return;
        };
        commands.entity(biped_entity).remove::<SeatedInVehicle>();
        game_objects::pawn::possess_pawn(conn_id, biped_entity, biped_net_id, registry, quic);
        game_objects::pawn::broadcast_seat_state(quic, biped_net_id, None);
        return;
    }

    if cockpit.occupant.is_some()
        || !vehicle_in_range(world, character, target, &cockpit, seat_transform)
    {
        return;
    }

    if enter_vehicle(world, character, target, &mut cockpit, seat_transform) {
        commands.entity(character).insert(SeatedInVehicle(target));
        game_objects::pawn::possess_pawn(conn_id, target, target_net_id, registry, quic);
        game_objects::pawn::broadcast_seat_state(quic, character_net_id, Some(target_net_id));
    }
}

fn handle_ability_pickup_interact(
    conn_id: ConnectionId,
    character: Entity,
    character_net_id: NetworkID,
    target: Entity,
    target_net_id: NetworkID,
    world: &PhysicsWorld,
    interactables: &Query<&game_objects::interaction::Interactable>,
    on_pickup_q: &Query<&OnPickup>,
    commands: &mut Commands,
    quic: &mut QuicManager,
    aim_dir: Vec3,
) -> bool {
    let Ok(&OnPickup(f)) = on_pickup_q.get(target) else {
        return false;
    };
    let Ok(interactable) = interactables.get(target) else {
        return true;
    };
    if !interactable_in_range(world, character, target, interactable.range) {
        return true;
    }
    f(character, target, aim_dir, commands);
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::AbilityPickup(character_net_id, target_net_id.clone()),
    );
    quic.send(SendTarget::All, Channel::Ordered, &MsgType::DespawnCommand(target_net_id));
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
    interactables: &Query<&game_objects::interaction::Interactable>,
    drop_dir: Vec3,
) {
    if held_weapons.0.contains_key(&target_net_id) {
        return;
    }
    let Ok(interactable) = interactables.get(target_entity) else {
        return;
    };
    if !interactable_in_range(world, player_entity, target_entity, interactable.range) {
        return;
    }
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let dropped = if slots.is_full() {
        let Some(dropped) = slots.remove_active() else {
            return;
        };
        Some(dropped)
    } else {
        None
    };
    drop(slots);
    if let Some((drop_id, drop_entity)) = dropped {
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
    }
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let _ = slots.assign_pickup(target_net_id.clone(), target_entity);
    held_weapons.0.insert(target_net_id.clone(), player_entity);
    game_objects::weapon::helpers::pickup_world_weapon(world, target_entity);
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::WeaponPickup(target_net_id, player_net_id),
    );
}

fn interactable_in_range(
    world: &PhysicsWorld,
    player_entity: Entity,
    target_entity: Entity,
    range: f32,
) -> bool {
    matches!(
        (body_position(world, player_entity), body_position(world, target_entity)),
        (Some(player_pos), Some(target_pos)) if player_pos.distance_squared(target_pos) <= range * range
    )
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
    let Some((player_entity, player_net_id)) = registry.character(conn_id) else {
        return;
    };
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let Some((weapon_id, weapon_entity)) = slots.remove_active() else {
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

pub(super) fn handle_drop_ability(
    conn_id: ConnectionId,
    registry: &PlayerRegistry,
    commands: &mut Commands,
    drop_dir: Vec3,
) {
    let Some((player_entity, _)) = registry.character(conn_id) else {
        return;
    };
    commands.queue(DropActiveAbility { owner: player_entity, aim_dir: drop_dir });
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
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
    quic: &mut QuicManager,
    tick: u64,
) {
    let Some((shooter_entity, _)) = registry.character(conn_id) else {
        return;
    };
    let shooter_holds =
        pawn_slots.get(shooter_entity).map(|s| s.contains_net_id(&weapon_net_id)).unwrap_or(false);
    if !shooter_holds {
        return;
    }
    let Some(weapon_entity) = find_networked_entity(all_networked, &weapon_net_id) else {
        return;
    };
    if !fire_weapon_authoritative(
        shooter_entity,
        weapon_entity,
        &weapon_net_id,
        kind,
        temp_id,
        origin,
        dir,
        pawn_slots,
        weapon_runtime,
        held_weapons,
        commands,
        world,
        net_ids,
        quic,
        tick,
        Some(conn_id),
    ) {
        if let Ok((weapon_state, _)) = weapon_runtime.get_mut(weapon_entity) {
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &MsgType::WeaponState(weapon_net_id, *weapon_state),
            );
        }
    }
}

pub(super) fn fire_weapon_authoritative(
    shooter_entity: Entity,
    weapon_entity: Entity,
    weapon_net_id: &NetworkID,
    kind: GameObjectKind,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
    quic: &mut QuicManager,
    tick: u64,
    owner_conn: Option<ConnectionId>,
) -> bool {
    crate::helpers::fire_weapon_authoritative(
        shooter_entity,
        weapon_entity,
        weapon_net_id,
        kind,
        temp_id,
        origin,
        dir,
        pawn_slots,
        weapon_runtime,
        held_weapons,
        commands,
        world,
        net_ids,
        Some(quic),
        tick,
        owner_conn,
    )
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
    let Some((shooter_entity, _)) = registry.character(conn_id) else {
        return;
    };
    let shooter_holds =
        pawn_slots.get(shooter_entity).map(|s| s.contains_net_id(&weapon_net_id)).unwrap_or(false);
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
        if started { SendTarget::All } else { SendTarget::One(conn_id) },
        Channel::Ordered,
        &MsgType::WeaponState(weapon_net_id, *weapon_state),
    );
}
