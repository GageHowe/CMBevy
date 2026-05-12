use bevy::{
    camera::{Camera, Camera3d, ClearColorConfig, PerspectiveProjection, Projection},
    color::Color,
    core_pipeline::{
        prepass::{DepthPrepass, NormalPrepass},
        tonemapping::Tonemapping,
    },
    math::Vec3,
    prelude::*,
};
// use bevy::core_pipeline::tonemapping::DebandDither::Enabled;
// use bevy::post_process::effect_stack::ChromaticAberration;

pub fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        // This render path uses SMAA plus prepass-driven post effects; keep MSAA off.
        Msaa::Off,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..Default::default()
        },
        AmbientLight {
            // MapMeta overrides this on map load; keep startup neutral so authored maps own it.
            brightness: 0.0,
            ..default()
        },
        // EnvironmentMapLight {
        //     diffuse_map: asset_server.load("textures/skyboxes/HDR_rich_blue_nebulae_1.ktx2"),
        //     specular_map: asset_server.load("textures/skyboxes/HDR_rich_blue_nebulae_1.ktx2"),
        //     intensity: 200.0,
        //     affects_lightmapped_mesh_diffuse: true,
        //     ..default()
        // },
        Projection::Perspective(PerspectiveProjection {
            fov: 90.0_f32.to_radians(),
            ..Default::default()
        }),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        // Skybox {
        //     image: asset_server.load("textures/skyboxes/HDR_rich_blue_nebulae_1.ktx2"),
        //     brightness: 1000.0,
        //     ..default()
        // },
        // Tonemapping::TonyMcMapface, // too washed out for me
        Tonemapping::AcesFitted, // punchy and dark/contrasty, maybe too much so
        // Tonemapping::Reinhard, // also washed out
        // Tonemapping::AgX, // good middle ground
        DepthPrepass,
        NormalPrepass,
        // MotionVectorPrepass, // required by MotionBlur and TAA
    ));
}
