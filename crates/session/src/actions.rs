use bevy::prelude::*;
use game_objects::{
    pawn::{
        HeldWeaponMap, PawnInputKind, PlayerRegistry, WeaponSlots,
        biped_ability::{DropActiveAbility, OnPickup},
        vehicle::*,
    },
    weapon::{WeaponConfig, WeaponState},
    *,
};
use net::{message::*, quic::*};
use physics::physics_world::*;

use crate::resources::*;

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
    rocket_turrets: &Query<&game_objects::pawn::RocketTurretPawnComponent>,
    interactables: &Query<&game_objects::interaction::Interactable>,
    mounts: &mut Query<&mut game_objects::pawn::CharacterMount>,
    mount_anchor_transforms: &Query<&Transform>,
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
    let Some(target) = all_networked.get(&target_net_id) else {
        return;
    };
    let aim_dir = pending_inputs
        .0
        .get(&conn_id)
        .map(|(_, input)| input)
        .and_then(|input| game_objects::pawn::aim_dir(world, character, Some(input)))
        .unwrap_or_else(|| body_forward(world, character));

    if vehicles.contains(target) {
        game_objects::pawn::vehicle::handle_server_interact(
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
            mounts,
            mount_anchor_transforms,
            commands,
        );
        return;
    }

    if rocket_turrets.contains(target) {
        let Ok(mut mount) = mounts.get_mut(target) else {
            return;
        };
        match game_objects::pawn::mount::handle_mount_interact(
            controlled,
            character,
            target,
            world,
            &mut mount,
            mount_anchor_transforms,
        ) {
            Some(game_objects::pawn::mount::MountInteractResult::Unmounted(biped_entity)) => {
                let Ok(biped_net_id) = net_ids.get(biped_entity) else {
                    return;
                };
                commands.entity(biped_entity).remove::<game_objects::pawn::Mounted>();
                game_objects::pawn::possess_pawn(conn_id, biped_entity, biped_net_id, registry, quic);
                game_objects::pawn::broadcast_mount_state(quic, biped_net_id, None);
            }
            Some(game_objects::pawn::mount::MountInteractResult::Mounted) => {
                commands.entity(character).insert(game_objects::pawn::Mounted(target));
                game_objects::pawn::possess_pawn(conn_id, target, &target_net_id, registry, quic);
                game_objects::pawn::broadcast_mount_state(
                    quic,
                    &character_net_id,
                    Some(&target_net_id),
                );
            }
            None => {}
        }
        return;
    }

    if game_objects::pawn::biped_ability::interact_pickup(
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

    game_objects::weapon::helpers::interact_pickup(
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
    world.body(entity).map(|rb| rb_rot(rb) * Vec3::NEG_Z).unwrap_or(Vec3::NEG_Z)
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
    game_objects::weapon::helpers::drop_from_owner(
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
    let Some(weapon_entity) = all_networked.get(&weapon_net_id) else {
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
    let Some(weapon_entity) = all_networked.get(&weapon_net_id) else {
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
