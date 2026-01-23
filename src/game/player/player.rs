use bevy::prelude::*;
// use bevy::window::PrimaryWindow;
// // use bevy_rapier3d::{plugin::RapierContext, prelude::QueryFilter};
use super::camera_controller;
use crate::game::physics::physics::*;
use rapier3d::prelude::*;
// use crate::game::{
//     level::targets::{DeadTarget, Target},
//     shooting,
// };
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // app.add_plugins(shooting::tracer::TracerPlugin)
        //     .add_systems(
        //         Update,
        //         (update_player, camera_controller::update_camera_controller),
        //     )
        app.add_systems(Startup, init_player)
            .add_systems(Update, camera_controller::update_camera_controller);
    }
}

#[derive(Component)]
pub struct Player {}

#[derive(Component)]
pub struct PlayerBodyHandle {
    pub handle: RigidBodyHandle,
}

fn init_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut physics: ResMut<PhysicsWorld>,
) {
    // let start_pos = Vector3::new(0.0, 2.0, 5.0);
    // let rb_handle = physics.spawn_capsule_player(start_pos);

    // // 2. Spawn a visible mesh for the body
    // let mesh_handle = meshes.add(Mesh::from(shape::Capsule {
    //     radius: 0.4,
    //     depth: 1.8,
    //     ..Default::default()
    // }));
    // let material_handle = materials.add(Color::rgb(0.2, 0.6, 1.0).into());

    // let player_transform = Transform::from_xyz(start_pos.x, start_pos.y, start_pos.z);

    commands.spawn((
        Player {},
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 120.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_xyz(0.0, 2.0, 5.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
        camera_controller::CameraController {
            sensitivity: 0.1,
            rotation: Vec2::ZERO,
            rotation_lock: 88.0,
        },
    ));
}
