use bevy::{prelude::*, transform::TransformSystems};
use rapier3d::prelude::*;

use super::*;
use crate::spawn::AppGameObjectExt;

#[cfg(feature = "client")]
mod controls;
mod lifecycle;
mod look;
mod melee;
mod movement;

#[cfg(feature = "client")]
pub(crate) use controls::consume_fixed_press;
#[cfg(feature = "client")]
pub use look::draw_biped_debug;
#[cfg(feature = "client")]
pub use look::draw_melee_debug;
pub use melee::{
    MELEE_DAMAGE, apply_melee_hits, melee_impulse, resolve_melee_hit, validate_melee_target,
};
pub use movement::{
    aim_pose, apply_biped_input, apply_biped_movement, biped_move_direction, viewmodel_offset,
};

pub const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

pub const CAPSULE_RADIUS: f32 = 0.3;
pub const CAPSULE_HALF_HEIGHT: f32 = 0.5;
pub const VIEW_PIVOT_OFFSET: Vec3 = Vec3::new(0.0, 0.4, 0.0);
pub(super) const SLIDE_HALF_HEIGHT: f32 = 0.1;
pub(super) const CAPSULE_BOTTOM: f32 = CAPSULE_HALF_HEIGHT + CAPSULE_RADIUS;
// Crouched capsule is top-aligned with standing: bottom = (CAPSULE_HALF_HEIGHT - SLIDE_HALF_HEIGHT) - SLIDE_HALF_HEIGHT - CAPSULE_RADIUS = 0.0
pub(super) const SLIDE_CAPSULE_BOTTOM: f32 = 0.0;
pub(super) const GROUND_ACCEL: f32 = 1.0;
/// m/s; impulse tapers to zero as speed approaches this
pub(super) const MAX_GROUND_SPEED: f32 = 8.0;
pub(super) const JUMP_IMPULSE: f32 = 8.0;
pub(super) const JUMP_IMPULSE_CROUCHED: f32 = 12.0;
pub(super) const CROUCH_DOWN_IMPULSE: f32 = 5.0;
pub(super) const AIR_CONTROL: f32 = 0.1;
pub(super) const GROUND_DIST: f32 = 0.05;
pub(super) const JUMP_COOLDOWN: u8 = 20;
pub(super) const MAIN_RESTITUTION: f32 = 0.0;
pub(super) const MAIN_FRICTION: f32 = 2.0;
pub(super) const SLIDE_FRICTION: f32 = 0.1;
pub(super) const BIPED_HEALTH_REGEN_PER_SEC: f32 = 5.0;

#[derive(Component, Default, Reflect)]
pub struct BipedPawnComponent {
    pub jump_cooldown: u8,
    pub look_yaw: f32,
    pub look_pitch: f32,
    #[reflect(ignore)]
    pub look_sync_dirty: bool,
    pub yaw_pivot: Option<Entity>,
    pub pitch_pivot: Option<Entity>,
    #[reflect(ignore)]
    pub collider: Option<ColliderHandle>,
    #[reflect(ignore)]
    pub ability: Option<crate::pawn::biped_ability::EquippedAbility>,
    #[cfg(feature = "client")]
    #[reflect(ignore)]
    pub jetpack_fx_entity: Option<Entity>,
    pub is_sliding: bool,
    pub melee_windup_ticks: u8,
    pub melee_cooldown_ticks: u8,
    #[reflect(ignore)]
    pub melee_fired_this_tick: bool,
    #[reflect(ignore)]
    pub melee_debug_start: Vec3,
    #[reflect(ignore)]
    pub melee_debug_end: Vec3,
    pub melee_debug_ticks: u8,
    pub snap_target: Option<Entity>,
    pub last_look_frame_body_rot: Option<Quat>,
}

pub struct BipedPlugin;
impl Plugin for BipedPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<BipedPawnComponent>()
            .init_resource::<MouseSensitivity>();
        app.add_systems(FixedUpdate, look::update_slide_camera);
        #[cfg(feature = "client")]
        {
            controls::configure(app);
            app.add_systems(
                FixedUpdate,
                melee::send_predicted_melee_hit.before(physics::physics_world::step_physics),
            );
        }
        app.add_systems(
            PostUpdate,
            (
                look::sync_remote_look_pivots,
                look::preserve_look_across_body_rotation,
            )
                .chain()
                .before(TransformSystems::Propagate),
        );
    }
}

#[derive(Component)]
pub struct YawPivot {
    pub yaw: f32,
}

#[derive(Component)]
pub struct PitchPivot {
    pub pitch: f32,
}
