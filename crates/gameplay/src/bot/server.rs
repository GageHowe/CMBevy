use bevy::prelude::*;
use common::tick::Ticker;
use net::{message::*, quic::*};
use physics::physics_world::*;

use crate::{Team, bot::*, health::Health, pawn::*, reticle::AimReticle, weapon::*};

pub fn run_bots(
    mut bots: Query<(Entity, &mut BotController)>,
    actors: Query<(Entity, &Team, &Health)>,
    smgs: Query<(), With<crate::weapon::smg::SmgComponent>>,
    mut pawn_slots: ParamSet<(Query<&mut WeaponSlots>, Query<&WeaponSlots>)>,
    mut weapon_runtime: Query<(&mut WeaponState, &WeaponConfig)>,
    reticles: Query<&AimReticle>,
    mut pawn_inputs: PawnInputQueries<'_, '_>,
    mut world: ResMut<PhysicsWorld>,
    mut quic: ResMut<QuicManager>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    mut held_weapons: ResMut<HeldWeaponMap>,
    tick: Res<Ticker>,
) {
    let actors = collect_contexts(&actors, &pawn_slots.p1(), &reticles, &world);
    drive_bots(
        &mut bots,
        &actors,
        &mut pawn_inputs,
        &mut world,
        |entity, fire, reload, aim_origin, aim_dir, world| {
            fire_active_weapon(
                entity,
                fire,
                aim_origin,
                aim_dir,
                &smgs,
                &mut pawn_slots.p0(),
                &mut weapon_runtime,
                &mut held_weapons,
                &mut commands,
                world,
                &mut net_ids,
                Some(&mut quic),
                tick.tick,
                #[cfg(feature = "client")]
                None,
            );
            if reload {
                reload_active_weapon(entity, &mut pawn_slots.p0(), &mut weapon_runtime);
            }
        },
    );
}

#[cfg(feature = "client")]
pub fn run_singleplayer_bots(
    mut bots: Query<(Entity, &mut BotController)>,
    actors: Query<(Entity, &Team, &Health)>,
    smgs: Query<(), With<crate::weapon::smg::SmgComponent>>,
    mut pawn_slots: ParamSet<(Query<&mut WeaponSlots>, Query<&WeaponSlots>)>,
    mut weapon_runtime: Query<(&mut WeaponState, &WeaponConfig)>,
    reticles: Query<&AimReticle>,
    mut beamers: Query<&mut crate::weapon::beamer::BeamerComponent>,
    mut pawn_inputs: PawnInputQueries<'_, '_>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut net_ids: ResMut<NetworkIDResource>,
    mut held_weapons: ResMut<HeldWeaponMap>,
    tick: Res<Ticker>,
) {
    let actors = collect_contexts(&actors, &pawn_slots.p1(), &reticles, &world);
    drive_bots(
        &mut bots,
        &actors,
        &mut pawn_inputs,
        &mut world,
        |entity, fire, reload, aim_origin, aim_dir, world| {
            fire_active_weapon(
                entity,
                fire,
                aim_origin,
                aim_dir,
                &smgs,
                &mut pawn_slots.p0(),
                &mut weapon_runtime,
                &mut held_weapons,
                &mut commands,
                world,
                &mut net_ids,
                None,
                tick.tick,
                Some(&mut beamers),
            );
            if reload {
                reload_active_weapon(entity, &mut pawn_slots.p0(), &mut weapon_runtime);
            }
        },
    );
}

type PawnInputQueries<'w, 's> = (
    Query<'w, 's, &'static mut BipedPawnComponent>,
    Query<'w, 's, &'static mut SpaceshipPawnComponent>,
    Query<'w, 's, &'static mut TruckPawnComponent>,
    Query<'w, 's, &'static mut HovercraftPawnComponent>,
);

fn drive_bots(
    bots: &mut Query<(Entity, &mut BotController)>,
    actors: &[BotContext],
    pawn_inputs: &mut PawnInputQueries<'_, '_>,
    world: &mut PhysicsWorld,
    mut on_output: impl FnMut(Entity, bool, bool, Vec3, Vec3, &mut PhysicsWorld),
) {
    for (entity, mut bot) in bots.iter_mut() {
        let Some(mut ctx) = actors.iter().find(|actor| actor.entity == entity).cloned() else {
            continue;
        };
        ctx.visible = actors.to_vec();
        let output = bot.brain.think(&ctx);
        let BotOutput {
            input,
            fire,
            reload,
            aim_origin,
            aim_dir,
        } = output;
        let _ = apply_server_input(
            entity,
            input,
            world,
            &mut pawn_inputs.0,
            &mut pawn_inputs.1,
            &mut pawn_inputs.2,
            &mut pawn_inputs.3,
        );
        on_output(entity, fire, reload, aim_origin, aim_dir, world);
    }
}

pub fn fire_active_weapon(
    shooter: Entity,
    want_fire: bool,
    origin: Vec3,
    dir: Vec3,
    smgs: &Query<(), With<crate::weapon::smg::SmgComponent>>,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
    quic: Option<&mut QuicManager>,
    tick: u64,
    #[cfg(feature = "client")] beamers: Option<
        &mut Query<&mut crate::weapon::beamer::BeamerComponent>,
    >,
) {
    #[cfg(not(feature = "client"))]
    let _ = tick;
    let Some((weapon_net_id, weapon_entity)) = active_weapon(shooter, pawn_slots) else {
        return;
    };
    #[cfg(feature = "client")]
    if let Some(beamers) = beamers {
        if beamers.contains(weapon_entity) {
            if want_fire {
                crate::weapon::beamer::tick_singleplayer_beam(
                    weapon_entity,
                    shooter,
                    origin,
                    dir,
                    tick,
                    beamers,
                    weapon_runtime,
                    commands,
                    world,
                );
            } else {
                crate::weapon::beamer::end_singleplayer_beam(
                    weapon_entity,
                    beamers,
                    weapon_runtime,
                );
            }
            return;
        }
    }
    if !want_fire || weapon_runtime.get_mut(weapon_entity).is_err() {
        return;
    }
    let _ = crate::weapon::fire_authoritative_with_replication(
        shooter,
        weapon_entity,
        &weapon_net_id,
        None,
        origin,
        spread_dir(smgs, weapon_entity, dir),
        pawn_slots,
        weapon_runtime,
        held_weapons,
        commands,
        world,
        net_ids,
        quic,
        None,
    );
}

pub fn reload_active_weapon(
    shooter: Entity,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
) {
    let Some((_, weapon_entity)) = active_weapon(shooter, pawn_slots) else {
        return;
    };
    let Ok((mut weapon_state, weapon_config)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    crate::weapon::start_reload(&mut weapon_state, weapon_config);
}

fn active_weapon(
    shooter: Entity,
    pawn_slots: &mut Query<&mut WeaponSlots>,
) -> Option<(NetworkID, Entity)> {
    pawn_slots.get_mut(shooter).ok()?.active_weapon()
}

fn spread_dir(
    smgs: &Query<(), With<crate::weapon::smg::SmgComponent>>,
    weapon_entity: Entity,
    dir: Vec3,
) -> Vec3 {
    if smgs.contains(weapon_entity) {
        crate::weapon::smg::spread_dir(dir)
    } else {
        dir
    }
}
