use bevy::prelude::*;
use common::tick::Ticker;
use physics::physics_world::PhysicsWorld;

use crate::{Team, bot::*, health::Health, pawn::*, reticle::AimReticle, weapon::WeaponFireInput};

pub fn run_bots(
    mut bots: Query<(Entity, &mut BotController, &mut Controller)>,
    actors: Query<(Entity, &Team, &Health)>,
    pawn_slots: Query<&WeaponSlots>,
    reticles: Query<&AimReticle>,
    world: Res<PhysicsWorld>,
    mut commands: Commands,
    tick: Res<Ticker>,
) {
    let actors = collect_contexts(&actors, &pawn_slots, &reticles, &world);
    for (entity, mut bot, mut possessed) in &mut bots {
        let Some(mut context) = actors.iter().find(|actor| actor.entity == entity).cloned() else {
            continue;
        };
        context.visible = actors.clone();
        let output = bot.brain.think(&context);
        possessed.push(output.input);
        let Ok(slots) = pawn_slots.get(entity) else {
            continue;
        };
        let Some(weapon) = slots.active().1 else {
            continue;
        };
        commands.entity(weapon).insert(WeaponFireInput {
            want_fire: output.fire,
            fire_pressed: output.fire,
            want_alt_fire: false,
            alt_fire_pressed: false,
            reload_pressed: output.reload,
            origin: output.aim_origin,
            aim_dir: output.aim_dir,
            shooter: entity,
            tick: tick.tick,
            prediction_id: tick.tick as u32,
        });
    }
}

pub use run_bots as run_singleplayer_bots;
