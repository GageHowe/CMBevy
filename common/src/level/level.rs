use bevy::prelude::*;

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, init_level);
    }
}

/// In the future, this will contain all information required to create a level.
/// It will be able to be passed over the network to other players.
pub struct LevelDescription {}

fn init_level(
    mut commands: Commands,
    // mut meshes: ResMut<Assets<Mesh>>,
    // mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // let level_material = materials.add(StandardMaterial {
    //     base_color: Color::WHITE,
    //     ..Default::default()
    // });
    // commands::spawn(())
    // spawn objects here
    // 
    // commands.spawn((
    //     DirectionalLight {
    //         illuminance: light_consts::lux::OVERCAST_DAY,
    //         shadows_enabled: true,
    //         ..Default::default()
    //     },
    //     Transform::from_xyz(0.0, 10.0, 0.0) // “location” of the sun
    //         .looking_at(Vec3::ZERO, Vec3::Y), // points at origin
    // ));
    // commands.spawn((
    //     Mesh3d(meshes.add(Cuboid::from_size(Vec3::splat(1.0)))),
    //     MeshMaterial3d(materials.add(Color::srgb(0.8, 0.7, 0.6))),
    //     Transform::from_xyz(0.0, 0.5, 0.0), // Position it
    //     Visibility::Visible,                // Ensure it is visible
    // ));
}
