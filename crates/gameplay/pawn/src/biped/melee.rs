use bevy::prelude::*;
#[cfg(feature = "client")]
use common::{LocalControl, PredictedImpulses};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_pos, rb_rot};

use super::*;
#[cfg(feature = "client")]
use crate::net::quic::Channel;
#[cfg(feature = "client")]
use crate::pawn::Possessed;
use crate::{
    health::{DamageCause, Health, LastDamageSource, attribute_damage},
    net::message::NetworkID,
};

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub(crate) struct PendingMeleeHits(
    pub std::collections::HashMap<crate::net::quic::ConnectionId, NetworkID>,
);

const MELEE_DELAY_TICKS: u8 = 8;
const MELEE_COOLDOWN_TICKS: u8 = 24;
const MELEE_RANGE: f32 = 1.5;
const MELEE_RADIUS: f32 = 0.3;
pub const MELEE_DAMAGE: f32 = 60.0;
const MELEE_IMPULSE: f32 = 0.8;
const MELEE_DEBUG_TICKS: u8 = 10;

pub fn tick_melee(
    world: &PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: BipedInput,
    biped: &mut BipedPawnComponent,
) {
    biped.melee_fired_this_tick = false;
    biped.melee_debug_ticks = biped.melee_debug_ticks.saturating_sub(1);
    biped.melee_cooldown_ticks = biped.melee_cooldown_ticks.saturating_sub(1);
    if biped.melee_windup_ticks > 0 {
        biped.melee_windup_ticks -= 1;
        if biped.melee_windup_ticks == 0 {
            if let Some((start, end)) =
                melee_segment(world, body_handle, input.look_yaw, input.look_pitch)
            {
                biped.melee_fired_this_tick = true;
                biped.melee_debug_start = start;
                biped.melee_debug_end = end;
                biped.melee_debug_ticks = MELEE_DEBUG_TICKS;
            }
        }
    }
    if input.melee_pressed && biped.melee_windup_ticks == 0 && biped.melee_cooldown_ticks == 0 {
        biped.melee_windup_ticks = MELEE_DELAY_TICKS;
        biped.melee_cooldown_ticks = MELEE_DELAY_TICKS.saturating_add(MELEE_COOLDOWN_TICKS);
    }
}

pub fn apply_melee_hits(
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<(Entity, &RigidBodyHandleComponent, &mut BipedPawnComponent)>,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
) {
    for (attacker, _body_handle, mut biped) in &mut bipeds {
        if !biped.melee_fired_this_tick {
            continue;
        }
        biped.melee_fired_this_tick = false;
        let start = biped.melee_debug_start;
        let end = biped.melee_debug_end;
        let impulse = melee_impulse(start, end);
        let dir = impulse.normalize_or_zero();
        let Some((victim, _, _, _)) =
            world.cast_sphere_ignoring_shields(start, dir, MELEE_RADIUS, MELEE_RANGE, &[attacker])
        else {
            continue;
        };
        world.apply_game_impulse(attacker, -impulse, None, None);
        if let Ok(mut health) = health_q.get_mut(victim) {
            attribute_damage(
                &mut last_damage_q,
                victim,
                Some(attacker),
                DamageCause::Unknown,
            );
            health.apply_damage(MELEE_DAMAGE);
        }
        world.apply_game_impulse(victim, impulse, None, None);
    }
}

#[cfg(not(feature = "client"))]
pub fn apply_melee_hit_requests(
    mut pending_melee_hits: ResMut<PendingMeleeHits>,
    registry: Res<crate::pawn::PlayerRegistry>,
    networked: Res<crate::NetworkEntityMap>,
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<&mut BipedPawnComponent>,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
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
        if !validate_melee_target(&mut world, attacker, target, start, end) {
            continue;
        }
        let impulse = melee_impulse(start, end);
        world.apply_game_impulse(attacker, -impulse, None, None);
        if let Ok(mut health) = health_q.get_mut(target) {
            attribute_damage(
                &mut last_damage_q,
                target,
                Some(attacker),
                DamageCause::Unknown,
            );
            health.apply_damage(MELEE_DAMAGE);
        }
        world.apply_game_impulse(target, impulse, None, None);
        biped.melee_debug_ticks = 0;
    }
}

#[cfg(feature = "client")]
pub fn send_predicted_melee_hit(
    mut world: ResMut<PhysicsWorld>,
    mut quic: Option<ResMut<crate::net::quic::QuicManager>>,
    control: Option<Res<LocalControl>>,
    mut impulses: Option<ResMut<PredictedImpulses>>,
    possessed: Query<(Entity, &BipedPawnComponent, &NetworkID), With<Possessed>>,
    net_ids: Query<&NetworkID>,
) {
    let Some(quic) = quic.as_deref_mut().filter(|quic| quic.client_connected) else {
        return;
    };
    let Ok((attacker, biped, attacker_net_id)) = possessed.single() else {
        return;
    };
    if !biped.melee_fired_this_tick {
        return;
    }
    let start = biped.melee_debug_start;
    let end = biped.melee_debug_end;
    let Some(victim) = resolve_melee_hit(&mut world, attacker, start, end) else {
        return;
    };
    let impulse = melee_impulse(start, end);
    world.apply_game_impulse(
        attacker,
        -impulse,
        Some(attacker_net_id),
        control.as_deref().zip(impulses.as_deref_mut()),
    );
    if let Ok(victim_net_id) = net_ids.get(victim) {
        quic.send_to_server(
            Channel::Ordered,
            &crate::net::message::MsgType::MeleeHitRequest(crate::net::message::MeleeHitRequest(
                victim_net_id.clone(),
            )),
        );
    }
}

#[cfg(feature = "client")]
pub fn melee_radius() -> f32 {
    MELEE_RADIUS
}

fn melee_segment(
    world: &PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    look_yaw: f32,
    look_pitch: f32,
) -> Option<(Vec3, Vec3)> {
    let body = world.rigid_body_set.get(body_handle.0)?;
    let body_rot = rb_rot(body);
    let body_pos = rb_pos(body);
    let camera_dir = body_rot
        * Quat::from_rotation_y(look_yaw)
        * Quat::from_rotation_x(look_pitch)
        * Vec3::NEG_Z;
    let up = body_rot * Vec3::Y;
    let start = body_pos + up * VIEW_PIVOT_OFFSET.y;
    let end = start + camera_dir.normalize_or_zero() * MELEE_RANGE;
    Some((start, end))
}

pub fn resolve_melee_hit(
    world: &mut PhysicsWorld,
    attacker: Entity,
    start: Vec3,
    end: Vec3,
) -> Option<Entity> {
    let dir = (end - start).normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    world
        .cast_sphere_ignoring_shields(start, dir, MELEE_RADIUS, MELEE_RANGE, &[attacker])
        .map(|(entity, _, _, _)| entity)
}

pub fn validate_melee_target(
    world: &mut PhysicsWorld,
    attacker: Entity,
    target: Entity,
    start: Vec3,
    end: Vec3,
) -> bool {
    resolve_melee_hit(world, attacker, start, end) == Some(target)
}

pub fn melee_impulse(start: Vec3, end: Vec3) -> Vec3 {
    let dir = (end - start).normalize_or_zero();
    let dir = if dir == Vec3::ZERO { Vec3::NEG_Z } else { dir };
    dir * MELEE_IMPULSE
}
