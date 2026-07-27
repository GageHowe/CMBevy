use bevy::prelude::*;
use physics::physics_world::*;

use super::*;

#[cfg(feature = "client")]
pub(super) fn sync_remote_look_pivots(
    bipeds: Query<&BipedPawnComponent, Without<Controller>>,
    mut pivots: ParamSet<(
        Query<(&mut Transform, &mut YawPivot)>,
        Query<(&mut Transform, &mut PitchPivot)>,
    )>,
) {
    for biped in &bipeds {
        if let Some(yaw_e) = biped.yaw_pivot
            && let Ok((mut transform, mut pivot)) = pivots.p0().get_mut(yaw_e)
        {
            pivot.yaw = biped.look_yaw;
            transform.rotation = Quat::from_rotation_y(biped.look_yaw);
        }
        if let Some(pitch_e) = biped.pitch_pivot
            && let Ok((mut transform, mut pivot)) = pivots.p1().get_mut(pitch_e)
        {
            pivot.pitch = biped.look_pitch;
            transform.rotation = Quat::from_rotation_x(biped.look_pitch);
        }
    }
}

#[cfg(not(feature = "client"))]
pub(super) fn sync_remote_look_pivots() {}

#[cfg(feature = "client")]
pub fn draw_biped_debug(
    world: Res<PhysicsWorld>,
    bipeds: Query<&RigidBodyHandleComponent, With<BipedPawnComponent>>,
    mut gizmos: Gizmos,
) {
    use physics::debug::draw_body_colliders;
    for body_handle in bipeds.iter() {
        draw_body_colliders(
            &world,
            body_handle,
            Color::srgba(0.3, 0.6, 1.0, 0.1),
            &mut gizmos,
        );
    }
}

#[cfg(feature = "client")]
pub fn draw_melee_debug(bipeds: Query<&BipedPawnComponent>, mut gizmos: Gizmos) {
    for biped in &bipeds {
        if biped.melee_debug_ticks == 0 {
            continue;
        }
        gizmos.line(
            biped.melee_debug_start,
            biped.melee_debug_end,
            Color::srgba(1.0, 0.2, 0.2, 0.9),
        );
        gizmos.sphere(
            Isometry3d::from_translation(biped.melee_debug_start),
            super::melee::melee_radius(),
            Color::srgba(1.0, 0.2, 0.2, 0.25),
        );
        gizmos.sphere(
            Isometry3d::from_translation(biped.melee_debug_end),
            super::melee::melee_radius(),
            Color::srgba(1.0, 0.2, 0.2, 0.25),
        );
    }
}

pub(super) fn update_slide_camera(
    bipeds: Query<&BipedPawnComponent>,
    mut pivots: Query<&mut Transform, With<YawPivot>>,
) {
    for biped in bipeds.iter() {
        let Some(yaw_e) = biped.yaw_pivot else {
            continue;
        };
        let Ok(mut t) = pivots.get_mut(yaw_e) else {
            continue;
        };
        // Fixed eye-level offset from the body origin. The crouched capsule is top-aligned
        // so the body physically falls on the ground rather than the camera being animated.
        t.translation.y = 0.4;
    }
}
