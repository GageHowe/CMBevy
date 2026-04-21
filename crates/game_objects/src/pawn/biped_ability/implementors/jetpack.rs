use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, rb_rot};
use rapier3d::prelude::Vector3;

use super::super::{AbilityFx, BipedAbility, BipedAbilityState, drain_meter};
use crate::GameObjectKind;

const THRUST: f32 = 0.5;
const DRAIN: f32 = 1.5;

#[derive(Component, Default, Reflect)]
pub struct JetpackAbility;

impl BipedAbility for JetpackAbility {
    const METER_MAX: f32 = 100.0;
    const METER_REGEN: f32 = 0.4;
    const KIND: GameObjectKind = GameObjectKind::Jetpack;
    const PICKUP_COLOR: (f32, f32, f32) = (0.2, 0.5, 1.0);

    fn apply_input(
        world: &mut PhysicsWorld,
        owner: Entity,
        input: common::BipedInput,
        state: &mut BipedAbilityState,
    ) -> Option<AbilityFx> {
        let was_active = state.active;
        state.active = input.ability1 && drain_meter(state, DRAIN);
        if !state.active {
            return (was_active != state.active).then_some(AbilityFx::Jetpack(false));
        }
        let Some(&handle) = world.entity_to_handle.get(&owner) else {
            state.active = false;
            return was_active.then_some(AbilityFx::Jetpack(false));
        };
        let (up, mass) = {
            let Some(rb) = world.rigid_body_set.get(handle) else {
                state.active = false;
                return was_active.then_some(AbilityFx::Jetpack(false));
            };
            (rb_rot(rb) * Vec3::Y, rb.mass())
        };
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            let impulse = up * THRUST * mass;
            rb.apply_impulse(Vector3::new(impulse.x, impulse.y, impulse.z), true);
        }
        (was_active != state.active).then_some(AbilityFx::Jetpack(true))
    }
}
