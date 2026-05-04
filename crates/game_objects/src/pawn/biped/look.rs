use bevy::prelude::*;
use physics::physics_world::*;

use super::*;

#[cfg(feature = "client")]
pub(super) fn sync_remote_look_pivots(
    bipeds: Query<&BipedPawnComponent, Without<Possessed>>,
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

pub(super) fn preserve_look_across_body_rotation(
    snap_comp: Option<Res<super::LookSnapCompensation>>,
    mut possessed: Query<(&RigidBodyHandleComponent, &mut BipedPawnComponent), With<Possessed>>,
    mut pivots: ParamSet<(
        Query<&Transform, (With<Possessed>, Without<YawPivot>, Without<PitchPivot>)>,
        Query<(&mut Transform, &mut YawPivot), Without<Possessed>>,
        Query<(&mut Transform, &mut PitchPivot), Without<Possessed>>,
    )>,
) {
    let Ok((_body_handle, mut biped)) = possessed.single_mut() else {
        return;
    };
    let body_rot = {
        let body_transforms = pivots.p0();
        let Ok(body_transform) = body_transforms.single() else {
            return;
        };
        body_transform.rotation
    };
    if !snap_comp.map_or(true, |enabled| enabled.0) {
        biped.last_look_frame_body_rot = Some(body_rot);
        return;
    }
    let Some(prev_body_rot) = biped.last_look_frame_body_rot else {
        biped.last_look_frame_body_rot = Some(body_rot);
        return;
    };
    if body_rot.dot(prev_body_rot).abs() > 0.999_999 {
        return;
    }
    let (Some(yaw_e), Some(pitch_e)) = (biped.yaw_pivot, biped.pitch_pivot) else {
        biped.last_look_frame_body_rot = Some(body_rot);
        return;
    };
    let yaw = {
        let mut yaw_query = pivots.p1();
        let Ok((_, yaw_pivot)) = yaw_query.get_mut(yaw_e) else {
            biped.last_look_frame_body_rot = Some(body_rot);
            return;
        };
        yaw_pivot.yaw
    };
    let pitch = {
        let mut pitch_query = pivots.p2();
        let Ok((_, pitch_pivot)) = pitch_query.get_mut(pitch_e) else {
            biped.last_look_frame_body_rot = Some(body_rot);
            return;
        };
        pitch_pivot.pitch
    };
    let world_forward =
        prev_body_rot * Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Vec3::NEG_Z;
    let local_forward = (body_rot.inverse() * world_forward).normalize_or_zero();
    if local_forward != Vec3::ZERO {
        let yaw = f32::atan2(-local_forward.x, -local_forward.z);
        let pitch = local_forward.y.clamp(-1.0, 1.0).asin().clamp(-PITCH_MAX, PITCH_MAX);
        {
            let mut yaw_query = pivots.p1();
            if let Ok((mut yaw_t, mut yaw_pivot)) = yaw_query.get_mut(yaw_e) {
                yaw_pivot.yaw = yaw;
                yaw_t.rotation = Quat::from_rotation_y(yaw);
            }
        }
        {
            let mut pitch_query = pivots.p2();
            if let Ok((mut pitch_t, mut pitch_pivot)) = pitch_query.get_mut(pitch_e) {
                pitch_pivot.pitch = pitch;
                pitch_t.rotation = Quat::from_rotation_x(pitch);
            }
        }
    }
    biped.last_look_frame_body_rot = Some(body_rot);
}

#[cfg(feature = "client")]
pub fn draw_biped_debug(
    world: Res<PhysicsWorld>,
    bipeds: Query<&RigidBodyHandleComponent, With<BipedPawnComponent>>,
    mut gizmos: Gizmos,
) {
    use physics::debug::draw_body_colliders;
    for body_handle in bipeds.iter() {
        draw_body_colliders(&world, body_handle, Color::srgba(0.3, 0.6, 1.0, 0.1), &mut gizmos);
    }
}

#[cfg(feature = "client")]
pub fn draw_melee_debug(bipeds: Query<&BipedPawnComponent>, mut gizmos: Gizmos) {
    for biped in &bipeds {
        if biped.melee_debug_ticks == 0 {
            continue;
        }
        gizmos.line(biped.melee_debug_start, biped.melee_debug_end, Color::srgba(1.0, 0.2, 0.2, 0.9));
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
