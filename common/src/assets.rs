use bevy::prelude::*;
use bevy::gltf::GltfAssetLabel;


fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Light
    commands.spawn(DirectionalLight {
        shadows_enabled: true,
        ..default()
    });

    // GLB model — note the #Scene0 label
    // commands.spawn(SceneRoot(
    //
    //     asset_server.load("my_model.glb#Scene0"),
    // ));
}

pub fn test_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        SceneRoot(
           asset_server.load("my_model.glb#Scene0"),
        ),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}