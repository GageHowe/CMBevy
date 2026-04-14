use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, rb_rot};
use rapier3d::prelude::Vector3;

use super::super::{BipedAbility, BipedAbilityCtx, drain_meter};
use crate::GameObjectKind;

const THRUST: f32 = 0.5;
const DRAIN: f32 = 1.5;

#[derive(Component, Default, Reflect)]
pub struct JetpackAbility;

impl BipedAbility for JetpackAbility {
    const MODEL_PATH: &'static str = "models/placeholder_jetpack.glb#Scene0";
    const ICON_PATH: &'static str = "textures/icons/jetpack.png";
    const COOLDOWN_TICKS: u16 = 0;
    const METER_MAX: f32 = 100.0;
    const METER_REGEN: f32 = 0.4;
    const KIND: GameObjectKind = GameObjectKind::Jetpack;
    const PICKUP_COLOR: (f32, f32, f32) = (0.2, 0.5, 1.0);

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        _commands: &mut Commands,
        ctx: &mut BipedAbilityCtx,
    ) {
        if !ctx.held || !drain_meter(ctx.state, DRAIN) {
            return;
        }
        let Some(&handle) = world.entity_to_handle.get(&ctx.owner) else {
            return;
        };
        let (up, mass) = {
            let Some(rb) = world.rigid_body_set.get(handle) else {
                return;
            };
            (rb_rot(rb) * Vec3::Y, rb.mass())
        };
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            let impulse = up * THRUST * mass;
            rb.apply_impulse(Vector3::new(impulse.x, impulse.y, impulse.z), true);
        }
    }
}
