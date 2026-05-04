use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_pos, rb_rot};
use rapier3d::prelude::{Capsule, Collider, ColliderHandle, Pose, QueryFilter};

use super::*;
use crate::health::{DamageCause, Health, LastDamageSource, attribute_damage};

const MELEE_DELAY_TICKS: u8 = 8;
const MELEE_COOLDOWN_TICKS: u8 = 24;
const MELEE_RANGE: f32 = 1.3;
const MELEE_RADIUS: f32 = 0.8;
const MELEE_DAMAGE: f32 = 35.0;
const MELEE_IMPULSE: f32 = 1.0;
const MELEE_DEBUG_TICKS: u8 = 2;

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
            if let Some((start, end)) = melee_segment(world, body_handle, input.look_yaw, input.look_pitch) {
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
    for (attacker, body_handle, mut biped) in &mut bipeds {
        if !biped.melee_fired_this_tick {
            continue;
        }
        biped.melee_fired_this_tick = false;
        let start = biped.melee_debug_start;
        let end = biped.melee_debug_end;
        let excluded = [body_handle.0];
        let pred = |_: ColliderHandle, col: &Collider| {
            !col.is_sensor() && col.parent().is_none_or(|rb_h| !excluded.contains(&rb_h))
        };
        let filter = QueryFilter::new().predicate(&pred);
        let qp = world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.rigid_body_set,
            &world.collider_set,
            filter,
        );
        let (shape, iso) = melee_capsule(start, end);
        let impulse = melee_impulse(start, end);
        let mut hit_entities = std::collections::HashSet::new();
        for (_, collider) in qp.intersect_shape(iso, &shape) {
            let Some(rb_handle) = collider.parent() else {
                continue;
            };
            let Some(&victim) = world.handle_to_entity.get(&rb_handle) else {
                continue;
            };
            if victim != attacker {
                hit_entities.insert(victim);
            }
        }
        if !hit_entities.is_empty() {
            world.apply_game_impulse(attacker, -impulse, None, None);
        }
        for victim in hit_entities {
            if let Ok(mut health) = health_q.get_mut(victim) {
                attribute_damage(&mut last_damage_q, victim, Some(attacker), DamageCause::Unknown);
                health.apply_damage(MELEE_DAMAGE);
            }
            world.apply_game_impulse(victim, impulse, None, None);
        }
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

fn melee_capsule(start: Vec3, end: Vec3) -> (Capsule, Pose) {
    let segment = end - start;
    let half_height = segment.length() * 0.5;
    let dir = segment.normalize_or_zero();
    let center = start.lerp(end, 0.5);
    (
        Capsule::new((-dir * half_height).into(), (dir * half_height).into(), MELEE_RADIUS),
        Pose::translation(center.x, center.y, center.z),
    )
}

fn melee_impulse(start: Vec3, end: Vec3) -> Vec3 {
    let dir = (end - start).normalize_or_zero();
    let dir = if dir == Vec3::ZERO { Vec3::NEG_Z } else { dir };
    dir * MELEE_IMPULSE
}
