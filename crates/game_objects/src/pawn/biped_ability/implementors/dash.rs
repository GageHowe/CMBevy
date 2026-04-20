use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, rb_rot};
use rapier3d::prelude::Vector3;

use super::super::{BipedAbility, BipedAbilityState, consume_charge};
use crate::{GameObjectKind, pawn::biped::biped_move_direction};

const DASH_IMPULSE: f32 = 14.0;

#[derive(Component, Default, Reflect)]
pub struct DashAbility;

impl BipedAbility for DashAbility {
    const COOLDOWN_TICKS: u16 = 20;
    const KIND: GameObjectKind = GameObjectKind::Dash;
    const PICKUP_COLOR: (f32, f32, f32) = (1.0, 0.8, 0.2);

    fn apply_input(
        world: &mut PhysicsWorld,
        owner: Entity,
        input: common::BipedInput,
        state: &mut BipedAbilityState,
    ) {
        if !input.ability1_pressed {
            return;
        }
        let Some(&handle) = world.entity_to_handle.get(&owner) else {
            return;
        };
        let (move_dir, mass) = {
            let Some(rb) = world.rigid_body_set.get(handle) else {
                return;
            };
            (biped_move_direction(rb_rot(rb), input), rb.mass())
        };
        if move_dir.length_squared() <= 1e-6
            || !consume_charge(state, Self::COOLDOWN_TICKS, Self::ACTIVE_TICKS)
        {
            return;
        }
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            let impulse = move_dir * DASH_IMPULSE * mass;
            rb.apply_impulse(Vector3::new(impulse.x, impulse.y, impulse.z), true);
        }
    }
}
