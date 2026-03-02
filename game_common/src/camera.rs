use bevy::prelude::*;
use bevy::anti_alias::smaa::Smaa;
use bevy::asset::AssetServer;
use bevy::camera::{Camera, Camera3d, ClearColorConfig, PerspectiveProjection, Projection};
use bevy::color::Color;
use bevy::math::Vec3;
use bevy::post_process::auto_exposure::AutoExposure;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::{default, Commands, Transform};
use bevy::core_pipeline::Skybox;

pub fn spawn_camera(mut commands: Commands, asset_server: Res<AssetServer>) {
    let skybox_handle = asset_server.load("textures/skyboxes/cubemap_rgba8.ktx2");
    commands.spawn((
        Camera3d::default(),
        // Camera::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..Default::default()
        },
        Smaa::default(),
        AutoExposure::default(),
        Bloom { // also includes hdr
            intensity: 0.05,
            ..default()
        },

        Projection::Perspective(PerspectiveProjection {
            // vertical FOV in radians
            fov: 90.0_f32.to_radians(),
            ..Default::default()
        }),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        Skybox {
            image: skybox_handle,
            brightness: 1000.0,
            ..default()
        },
    ));
}