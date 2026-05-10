use bevy::{
    anti_alias::smaa::Smaa,
    asset::AssetServer,
    camera::{Camera, Camera3d, ClearColorConfig, PerspectiveProjection, Projection},
    color::Color,
    core_pipeline::{
        Skybox,
        prepass::{DepthPrepass, NormalPrepass},
        tonemapping::Tonemapping,
    },
    math::Vec3,
    pbr::ScreenSpaceAmbientOcclusion,
    post_process::{
        auto_exposure::AutoExposure,
        bloom::{Bloom, BloomCompositeMode},
    },
    prelude::*,
};

use crate::color_compression::ColorCompressionSettings;
// use bevy::core_pipeline::tonemapping::DebandDither::Enabled;
// use bevy::post_process::effect_stack::ChromaticAberration;

pub fn spawn_camera(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands
        .spawn((
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
            EnvironmentMapLight {
                diffuse_map: asset_server.load("textures/skyboxes/HDR_rich_blue_nebulae_1.ktx2"),
                specular_map: asset_server.load("textures/skyboxes/HDR_rich_blue_nebulae_1.ktx2"),
                intensity: 200.0,
                affects_lightmapped_mesh_diffuse: true,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: 90.0_f32.to_radians(),
                ..Default::default()
            }),
            Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
            Skybox {
                image: asset_server.load("textures/skyboxes/HDR_rich_blue_nebulae_1.ktx2"),
                brightness: 1000.0,
                ..default()
            },
            // Tonemapping::TonyMcMapface, // too washed out for me
            Tonemapping::AcesFitted, // punchy and dark/contrasty, maybe too much so
            // Tonemapping::Reinhard, // also washed out
            // Tonemapping::AgX, // good middle ground
            DepthPrepass,
            NormalPrepass,
            // MotionVectorPrepass, // required by MotionBlur and TAA

            // post — nested to stay within Bevy's 16-item bundle arity limit
            (
                Smaa::default(),
                AutoExposure {
                    range: -12.0..=4.0,
                    speed_brighten: 0.2,
                    speed_darken: 0.1,
                    filter: 0.0..=0.50, // ignore 50% brightest pixels
                    ..default()
                },
                Bloom {
                    intensity: 0.4,
                    composite_mode: BloomCompositeMode::Additive, // additive = more sci-fi glow
                    high_pass_frequency: 0.5,                     // width of bloom
                    // prefilter: BloomPrefilter { threshold: 0.5, threshold_softness: 0.2 }, // only bloom true HDR content
                    ..default()
                },
                // SSAO — darkens crevices and contact shadows. Adds a lot of depth.
                // Quality: Low / Medium / High / Ultra
                ScreenSpaceAmbientOcclusion {
                    quality_level: bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::Medium,
                    ..default()
                },
                // ScreenSpaceReflections — disabled: requires DeferredPrepass which prevents depth copy to prepass texture
                // MotionBlur { shutter_angle: 0.5, samples: 4, ..default() },
                // ContrastAdaptiveSharpening { sharpening_strength: 0.6, ..default() },
                // ChromaticAberration::default(),
            ),
        ))
        .insert(ColorCompressionSettings::default())
        .insert(crate::outline::OutlineSettings {
            threshold: 0.10,
            color: Vec4::new(0.5, 0.5, 0.5, 0.03),
        });
}
