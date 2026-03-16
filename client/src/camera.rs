use bevy::prelude::*;
use bevy::anti_alias::smaa::Smaa;
use bevy::asset::AssetServer;
use bevy::camera::{Camera, Camera3d, ClearColorConfig, PerspectiveProjection, Projection};
use bevy::color::Color;
use bevy::math::Vec3;
use bevy::post_process::auto_exposure::AutoExposure;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::core_pipeline::Skybox;
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};

pub fn spawn_camera(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Camera3d::default(),
        Msaa::Off,
        Camera {
            // hdr: true,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..Default::default()
        },
        AmbientLight { brightness: 0.0, ..default() },
        EnvironmentMapLight {
            diffuse_map: asset_server.load("textures/HDR_rich_blue_nebulae_1.ktx2"),
            specular_map: asset_server.load("textures/HDR_rich_blue_nebulae_1.ktx2"),
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
            image: asset_server.load("textures/HDR_rich_blue_nebulae_1.ktx2"),
            brightness: 1000.0,
            ..default()
        },

        // tonemapping — override the Camera3d default (ReinhardLuminance).
        // AgX: neutral, filmic, good for HDR. TonyMcMapface: more contrasty/stylized.
        // BlenderFilmic: similar to ACES but less harsh. AcesFitted: punchy, saturated.
        Tonemapping::AcesFitted,

        // prepasses — required by SSAO and outline
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
                filter: 0.0..=0.50, // ignore 50% brighest pixels
                ..default()
            },
            Bloom {
                intensity: 0.4,
                composite_mode: BloomCompositeMode::Additive, // additive = more sci-fi glow
                high_pass_frequency: 0.5, // width of bloom
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
        ),
    ))
    .insert(crate::outline::OutlineSettings {
        threshold: 0.05,
        color: Vec4::new(0.5, 0.5, 0.5, 0.05),
    });
}
