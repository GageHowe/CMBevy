use bevy::prelude::*;
use common::tick::Ticker;
use game_objects::{Team, bot::*, health::Health, pawn::*, weapon::*};
use net::{message::*, quic::*};
use physics::physics_world::*;

pub(super) fn run_bots(
    mut bots: Query<(Entity, &mut BotController)>,
    actors: Query<(Entity, &Team, &Health)>,
    mut pawn_slots: Query<&mut WeaponSlots>,
    mut weapon_runtime: Query<(&mut WeaponState, &WeaponConfig)>,
    mut pawns: PawnInputParams,
    mut world: ResMut<PhysicsWorld>,
    mut quic: ResMut<QuicManager>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    mut held_weapons: ResMut<game_objects::pawn::HeldWeaponMap>,
    tick: Res<Ticker>,
) {
    let actors = collect_contexts(&actors, &world);
    for (entity, mut bot) in &mut bots {
        let Some(mut ctx) = actors.iter().find(|actor| actor.entity == entity).cloned() else {
            continue;
        };
        ctx.visible = actors.clone();
        let output = bot.brain.think(&ctx);
        let _ = pawns.apply_server_input(entity, output.input, &mut world);
        if output.fire {
            fire_active_weapon(
                entity,
                output.aim_origin,
                output.aim_dir,
                bot.next_temp_id(),
                &mut pawn_slots,
                &mut weapon_runtime,
                &mut held_weapons,
                &mut commands,
                &mut world,
                &mut net_ids,
                &mut quic,
                tick.tick,
            );
        }
        if output.reload {
            reload_active_weapon(entity, &mut pawn_slots, &mut weapon_runtime, &mut quic);
        }
    }
}

pub(super) fn fire_active_weapon(
    shooter: Entity,
    origin: Vec3,
    dir: Vec3,
    temp_id: u32,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut game_objects::pawn::HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
    quic: &mut QuicManager,
    tick: u64,
) {
    let Some((weapon_net_id, weapon_entity)) = ({
        let Ok(slots) = pawn_slots.get_mut(shooter) else {
            return;
        };
        slots.active_weapon()
    }) else {
        return;
    };
    let Ok((_, config)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    let kind = config.projectile_kind.clone();
    game_objects::weapon::fire_authoritative_with_replication(
        shooter,
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
        Some(quic),
        tick,
        None,
    );
}

pub(super) fn reload_active_weapon(
    shooter: Entity,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    quic: &mut QuicManager,
) {
    let Some((weapon_net_id, weapon_entity)) = ({
        let Ok(slots) = pawn_slots.get_mut(shooter) else {
            return;
        };
        slots.active_weapon()
    }) else {
        return;
    };
    let Ok((mut weapon_state, weapon_config)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    game_objects::weapon::start_reload(&mut weapon_state, weapon_config);
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::WeaponState(weapon_net_id, *weapon_state),
    );
}
