use bevy::{prelude::*, transform::TransformSystems};
use physics::physics_world::{PhysicsWorld, sync_physics_visual};

pub struct SpringArmPlugin;
impl Plugin for SpringArmPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "client")]
        app.add_systems(
            PostUpdate,
            update_spring_arms
                .after(sync_physics_visual)
                .before(TransformSystems::Propagate),
        );
    }
}

/// Marker placed on the intermediate spring arm entity so it can be found and despawned
/// when the camera detaches from a vehicle.
#[derive(Component)]
pub struct SpringArmPivot;

/// Attach this component to an intermediate entity (child of a pawn, parent of the camera).
/// Each frame the entity's local translation is set to the arm direction × current length,
/// keeping the camera clear of geometry without any coupling to CameraEffector.
///
/// Hierarchy: vehicle → [SpringArm entity] → camera (CameraEffector.base_translation = ZERO)
#[derive(Component)]
pub struct SpringArm {
    /// Desired local offset from the parent (direction × length).
    pub offset: Vec3,
    /// Gap kept between the camera and any surface it collides with.
    pub probe_radius: f32,
    /// How fast the arm recovers to full length after clearing geometry (lerp t/sec).
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

        // Ray cast from the parent toward the desired camera position. A ray avoids the
        // sphere-cast initial-overlap problem (starting inside geometry returns toi=0),
        // which caused the camera to collapse to the body origin when inverted near the floor.
        // probe_radius is subtracted from the hit distance to keep the camera off surfaces.
        let new_length = match world.cast_ray(parent_pos, arm_dir_world, max_length, &[parent_entity]) {
            Some((_entity, hit_t)) => (hit_t - arm.probe_radius).max(0.0),
            None => {
                let t = (arm.recover_speed * time.delta_secs()).min(1.0);
                arm.current_length + (max_length - arm.current_length) * t
            }
        };

        arm.current_length = new_length;
        // Update this entity's local translation; Bevy's transform propagation carries it
        // through to the camera child, so CameraEffector needs no knowledge of the spring arm.
        local_t.translation = arm_dir_local * new_length;
    }
}
