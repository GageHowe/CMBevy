use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, rb_rot};
use rapier3d::prelude::Vector3;

use super::super::{AbilityFx, BipedAbility, BipedAbilityState, drain_meter};
use crate::GameObjectKind;

const DASH_IMPULSE: f32 = 14.0;
const DASH_COST: f32 = 50.0;

#[derive(Component, Default, Reflect)]
pub struct DashAbility;

impl BipedAbility for DashAbility {
    const METER_MAX: f32 = 100.0;
    const METER_REGEN: f32 = 1.0;
    const KIND: GameObjectKind = GameObjectKind::Dash;
    const PICKUP_COLOR: (f32, f32, f32) = (1.0, 0.8, 0.2);

    fn apply_input(
        world: &mut PhysicsWorld,
        owner: Entity,
        input: common::BipedInput,
        state: &mut BipedAbilityState,
    ) -> Option<AbilityFx> {
        if !input.ability1_pressed {
            return None;
        }
        let Some(&handle) = world.entity_to_handle.get(&owner) else {
            return None;
        };
        let (dash_dir, mass) = {
            let Some(rb) = world.rigid_body_set.get(handle) else {
                return None;
            };
            let facing = rb_rot(rb) * Quat::from_rotation_y(input.look_yaw) * Vec3::NEG_Z;
            let move_dir = (facing * input.forward
                + (rb_rot(rb) * Quat::from_rotation_y(input.look_yaw) * Vec3::X) * input.right)
                .normalize_or_zero();
            let dash_dir = if move_dir.length_squared() > 1e-6 {
                move_dir
            } else {
                facing.normalize_or_zero()
            };
            (dash_dir, rb.mass())
        };
        if dash_dir.length_squared() <= 1e-6 || !drain_meter(state, DASH_COST) {
            return None;
        }
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            let impulse = dash_dir * DASH_IMPULSE * mass;
            rb.apply_impulse(Vector3::new(impulse.x, impulse.y, impulse.z), true);
        }
        Some(AbilityFx::Dash(dash_dir))
    }
}
