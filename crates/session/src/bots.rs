use bevy::prelude::*;
use common::tick::Ticker;
use gameplay::{Team, bot::*, health::Health, pawn::*, reticle::AimReticle, weapon::*};
use net::{message::*, quic::*};
use physics::physics_world::*;

pub(super) fn run_bots(
    mut bots: Query<(Entity, &mut BotController)>,
    actors: Query<(Entity, &Team, &Health)>,
    mut pawn_slots: ParamSet<(Query<&mut WeaponSlots>, Query<&WeaponSlots>)>,
    smgs: Query<(), With<gameplay::weapon::smg::SmgComponent>>,
    mut weapon_runtime: Query<(&mut WeaponState, &WeaponConfig)>,
    reticles: Query<&AimReticle>,
    mut pawn_inputs: (
        Query<&mut BipedPawnComponent>,
        Query<&mut SpaceshipPawnComponent>,
        Query<&mut TruckPawnComponent>,
        Query<&mut HovercraftPawnComponent>,
    ),
    mut world: ResMut<PhysicsWorld>,
    mut quic: ResMut<QuicManager>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    mut held_weapons: ResMut<gameplay::pawn::HeldWeaponMap>,
    tick: Res<Ticker>,
) {
    let actors = collect_contexts(&actors, &pawn_slots.p1(), &reticles, &world);
    for (entity, mut bot) in &mut bots {
        let Some(mut ctx) = actors.iter().find(|actor| actor.entity == entity).cloned() else {
            continue;
        };
        ctx.visible = actors.clone();
        let output = bot.brain.think(&ctx);
        let _ = apply_server_input(
            entity,
            output.input,
            &mut world,
            &mut pawn_inputs.0,
            &mut pawn_inputs.1,
            &mut pawn_inputs.2,
            &mut pawn_inputs.3,
        );
        if output.fire {
            fire_active_weapon(
                entity,
                output.aim_origin,
                output.aim_dir,
                bot.next_temp_id(),
                &smgs,
                &mut pawn_slots.p0(),
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
            reload_active_weapon(entity, &mut pawn_slots.p0(), &mut weapon_runtime, &mut quic);
        }
    }
}

pub(super) fn fire_active_weapon(
    shooter: Entity,
    origin: Vec3,
    dir: Vec3,
    temp_id: u32,
    smgs: &Query<(), With<gameplay::weapon::smg::SmgComponent>>,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut gameplay::pawn::HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
    quic: &mut QuicManager,
    _tick: u64,
) {
    let Some((weapon_net_id, weapon_entity)) = ({
        let Ok(slots) = pawn_slots.get_mut(shooter) else {
            return;
        };
        slots.active_weapon()
    }) else {
        return;
    };
    let Ok((_, _)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    let dir = if smgs.contains(weapon_entity) {
        gameplay::weapon::smg::spread_dir(dir)
    } else {
        dir
    };
    gameplay::weapon::fire_authoritative_with_replication(
        shooter,
        weapon_entity,
        &weapon_net_id,
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
        None,
    );
}

pub(super) fn reload_active_weapon(
    shooter: Entity,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    _quic: &mut QuicManager,
) {
    let Some((_weapon_net_id, weapon_entity)) = ({
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
    gameplay::weapon::start_reload(&mut weapon_state, weapon_config);
}
