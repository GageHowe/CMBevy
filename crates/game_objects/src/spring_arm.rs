use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, sync_physics_visual};

pub struct SpringArmPlugin;
impl Plugin for SpringArmPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "client")]
        app.add_systems(PostUpdate, update_spring_arms.after(sync_physics_visual));
    }
}

/// Attach to a camera entity (as a child of a pawn) to prevent it from clipping into geometry.
/// Each frame it sphere-casts from the parent toward the desired offset; if geometry is in the
/// way the camera is pulled in, and it smoothly recovers to the full length when clear.
#[derive(Component)]
pub struct SpringArm {
    /// Desired local offset from the parent (direction × length).
    pub offset: Vec3,
    /// Sphere probe radius — keeps the camera surface clear of geometry surfaces.
    pub probe_radius: f32,
    /// How fast the arm extends back to full length after clearing geometry (lerp t/sec).
    pub recover_speed: f32,
    #[cfg(feature = "client")]
    current_length: f32,
}

impl SpringArm {
    pub fn new(offset: Vec3, probe_radius: f32, recover_speed: f32) -> Self {
        Self {
            offset,
            probe_radius,
            recover_speed,
            #[cfg(feature = "client")]
            current_length: offset.length(),
        }
    }
}

#[cfg(feature = "client")]
fn update_spring_arms(
    world: Res<PhysicsWorld>,
    time: Res<Time>,
    parent_transforms: Query<&GlobalTransform>,
    mut arms: Query<(&ChildOf, &mut Transform, &mut SpringArm)>,
) {
    for (child_of, mut local_t, mut arm) in arms.iter_mut() {
        let max_length = arm.offset.length();
        if max_length < 1e-6 {
            continue;
        }

        let parent_entity = child_of.parent();
        let Ok(parent_gt) = parent_transforms.get(parent_entity) else {
            continue;
        };
        let (_, parent_rot, parent_pos) = parent_gt.to_scale_rotation_translation();

        let arm_dir_local = arm.offset / max_length;
        let arm_dir_world = parent_rot * arm_dir_local;
        let desired_pos = parent_pos + arm_dir_world * max_length;

        // Cast from the desired camera position back toward the vehicle, not forward from the
        // vehicle. Casting forward hits the ground when the vehicle is upside-down; casting
        // backward only shortens the arm when something is actually between the camera and body.
        let new_length = match world.cast_sphere(
            desired_pos,
            -arm_dir_world,
            arm.probe_radius,
            max_length,
            &[parent_entity],
        ) {
            // Hit at distance d from the desired end: pull the camera in by that amount.
            Some((_entity, hit_t, _normal)) => (max_length - hit_t).max(0.0),
            // Clear: smoothly recover toward the full arm length.
            None => {
                let t = (arm.recover_speed * time.delta_secs()).min(1.0);
                arm.current_length + (max_length - arm.current_length) * t
            }
        };

        arm.current_length = new_length;
        local_t.translation = arm_dir_local * new_length;
    }
}
