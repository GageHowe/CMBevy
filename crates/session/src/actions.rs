use bevy::prelude::*;
use game_objects::{
    pawn::{
        HeldWeaponMap, PawnInputKind, PlayerRegistry, WeaponSlots,
        biped_ability::OnPickup,
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

pub(super) fn handle_melee_hit_request(
    conn_id: ConnectionId,
    target_net_id: NetworkID,
    pending_melee_hits: &mut PendingMeleeHits,
) {
    pending_melee_hits.0.insert(conn_id, target_net_id);
}

pub(super) fn apply_melee_hit_requests(
    mut pending_melee_hits: ResMut<PendingMeleeHits>,
    registry: Res<PlayerRegistry>,
    networked: Res<NetworkEntityMap>,
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<&mut game_objects::pawn::BipedPawnComponent>,
    mut health_q: Query<&mut game_objects::health::Health>,
    mut last_damage_q: Query<&mut game_objects::health::LastDamageSource>,
) {
    let requests = std::mem::take(&mut pending_melee_hits.0);
    for (conn_id, target_net_id) in requests {
        let Some((attacker, _)) = registry.character(conn_id) else {
            continue;
        };
        let Some(target) = networked.get(&target_net_id) else {
            continue;
        };
        let Ok(mut biped) = bipeds.get_mut(attacker) else {
            continue;
        };
        if biped.melee_debug_ticks == 0 {
            continue;
        }
        let start = biped.melee_debug_start;
        let end = biped.melee_debug_end;
        if !game_objects::pawn::biped::validate_melee_target(&mut world, attacker, target, start, end)
        {
            continue;
        }
        let impulse = game_objects::pawn::biped::melee_impulse(start, end);
        world.apply_game_impulse(attacker, -impulse, None, None);
        if let Ok(mut health) = health_q.get_mut(target) {
            game_objects::health::attribute_damage(
                &mut last_damage_q,
                target,
                Some(attacker),
                game_objects::health::DamageCause::Unknown,
            );
            health.apply_damage(game_objects::pawn::biped::MELEE_DAMAGE);
        }
        world.apply_game_impulse(target, impulse, None, None);
        biped.melee_debug_ticks = 0;
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
        game_objects::pawn::mount::handle_server_interact(
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
            mounts,
            mount_anchor_transforms,
            commands,
        );
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

    game_objects::weapon::handle_interact_pickup_request(
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

pub(super) fn handle_drop_ability(
    conn_id: ConnectionId,
    registry: &PlayerRegistry,
    commands: &mut Commands,
    drop_dir: Vec3,
) {
    game_objects::pawn::biped_ability::handle_drop_request(conn_id, registry, commands, drop_dir);
}
