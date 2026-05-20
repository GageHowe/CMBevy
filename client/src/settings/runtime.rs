use std::time::{Duration, Instant};

use bevy::{
    core_pipeline::prepass::MotionVectorPrepass,
    post_process::motion_blur::MotionBlur,
    prelude::*,
    render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
    solari::prelude::SolariLighting,
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode},
};
use bevy_egui::{EguiContextSettings, PrimaryEguiContext};
use common::{ActiveBindings, PromptDevicePreference};
use game_objects::pawn::{CameraEffector, LookSnapCompensation, MouseSensitivity};
use physics::physics_world::PhysicsInterpMode;

use super::data::{DisplayMode, PhysicsInterp, Settings, ShadowQuality, SsaoQuality, VsyncMode};
use crate::{color_compression::ColorCompressionSettings, outline::OutlineSettings};

pub fn apply_settings(
    mut commands: Commands,
    settings: Res<Settings>,
    mut sensitivity: ResMut<MouseSensitivity>,
    mut snap_comp: ResMut<LookSnapCompensation>,
    mut interp_mode: ResMut<PhysicsInterpMode>,
    mut cam_effects: Query<(Entity, &mut CameraEffector), With<Camera3d>>,
    mut window_q: Query<&mut Window, With<PrimaryWindow>>,
    mut directional_lights: Query<&mut DirectionalLight>,
    mut directional_light_shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
    mut egui_context_settings: Query<&mut EguiContextSettings, With<PrimaryEguiContext>>,
) {
    commands.insert_resource(PromptDevicePreference(settings.prompt_device_mode));
    sensitivity.base = settings.mouse_sensitivity;
    sensitivity.zoom_blend = settings.zoom_sensitivity_blend;
    sensitivity.vehicle_pitch_yaw = settings.vehicle_pitch_yaw_sensitivity;
    sensitivity.gamepad_look = settings.gamepad_look_sensitivity;
    sensitivity.gamepad_move_deadzone = settings.gamepad_move_deadzone;
    sensitivity.gamepad_look_deadzone = settings.gamepad_look_deadzone;
    sensitivity.gamepad_invert_y = settings.gamepad_invert_y;
    snap_comp.0 = settings.preserve_look_across_planet_snap;

    if let Ok(mut window) = window_q.single_mut() {
        window.present_mode = match settings.vsync {
            VsyncMode::AutoVsync => PresentMode::AutoVsync,
            VsyncMode::AutoNoVsync => PresentMode::AutoNoVsync,
            VsyncMode::Fifo => PresentMode::Fifo,
            VsyncMode::FifoRelaxed => PresentMode::FifoRelaxed,
            VsyncMode::Immediate => PresentMode::Immediate,
            VsyncMode::Mailbox => PresentMode::Mailbox,
        };
        window.mode = match settings.display_mode {
            DisplayMode::Windowed => WindowMode::Windowed,
            DisplayMode::BorderlessFullscreen => {
                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
            }
        };
    }

    if let Ok((camera_entity, mut fx)) = cam_effects.single_mut() {
        fx.base_fov = settings.fov;
        fx.current_fov = settings.fov;

        let mut camera = commands.entity(camera_entity);
        apply_camera_graphics(&mut camera, &settings);
    }

    *interp_mode = match settings.physics_interp {
        PhysicsInterp::Off => PhysicsInterpMode::Off,
        PhysicsInterp::Interpolate => PhysicsInterpMode::Interpolate,
        PhysicsInterp::Extrapolate => PhysicsInterpMode::Extrapolate,
        PhysicsInterp::Balanced => PhysicsInterpMode::Balanced,
    };
    if let Ok(mut egui_settings) = egui_context_settings.single_mut() {
        egui_settings.scale_factor = settings.ui_scale;
    }

    directional_light_shadow_map.size = shadow_map_size(&settings.shadow_quality);
    for mut directional_light in &mut directional_lights {
        directional_light.shadows_enabled =
            !settings.raytracing && !matches!(settings.shadow_quality, ShadowQuality::Off);
    }
}

pub fn sync_dynamic_graphics_settings(
    mut commands: Commands,
    settings: Option<Res<Settings>>,
    added_cameras: Query<Entity, Added<Camera3d>>,
    // mut added_directional_lights: Query<&mut DirectionalLight, Added<DirectionalLight>>,
    _directional_light_shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
) {
    let Some(settings) = settings else {
        return;
    };

    for camera_entity in &added_cameras {
        let mut camera = commands.entity(camera_entity);
        apply_camera_graphics(&mut camera, &settings);
    }
}

fn apply_camera_graphics(camera: &mut EntityCommands, settings: &Settings) {
    if settings.anti_aliasing {
        camera.insert(bevy::anti_alias::smaa::Smaa::default());
    } else {
        camera.remove::<bevy::anti_alias::smaa::Smaa>();
    }

    if settings.raytracing {
        camera.insert(SolariLighting::default());
    } else {
        camera.remove::<SolariLighting>();
    }

    if settings.auto_exposure {
        camera.insert(bevy::post_process::auto_exposure::AutoExposure {
            range: -2.0..=0.0,
            // speed_brighten: 1.5,
            filter: 0.0..=0.99,
            // filter: 0.10..=0.90,
            // speed_darken: 0.75,
            ..default()
        });
    } else {
        camera.remove::<bevy::post_process::auto_exposure::AutoExposure>();
    }

    if settings.bloom {
        camera.insert(bevy::post_process::bloom::Bloom {
            intensity: settings.bloom_intensity.clamp(0.0, 2.0),
            composite_mode: bevy::post_process::bloom::BloomCompositeMode::Additive,
            high_pass_frequency: 0.5,
            prefilter: bevy::post_process::bloom::BloomPrefilter {
                threshold: settings.bloom_threshold.clamp(0.0, 5.0),
                threshold_softness: 0.0,
            },
            ..default()
        });
    } else {
        camera.remove::<bevy::post_process::bloom::Bloom>();
    }

    if settings.motion_blur {
        camera.insert(MotionVectorPrepass);
        camera.insert(MotionBlur {
            shutter_angle: settings.motion_blur_shutter_angle,
            samples: 1,
        });
    } else {
        camera.remove::<MotionBlur>();
        camera.remove::<MotionVectorPrepass>();
    }

    if settings.color_compression {
        camera.insert(ColorCompressionSettings {
            color_steps: settings.color_compression_steps.clamp(2.0, 64.0),
            dither_strength: settings.color_compression_dither.clamp(0.0, 1.5),
        });
    } else {
        camera.remove::<ColorCompressionSettings>();
    }

    if settings.raytracing {
        camera.remove::<bevy::pbr::ScreenSpaceAmbientOcclusion>();
    } else {
        match settings.ssao_quality {
            SsaoQuality::Off => {
                camera.remove::<bevy::pbr::ScreenSpaceAmbientOcclusion>();
            }
            SsaoQuality::Medium => {
                camera.insert(bevy::pbr::ScreenSpaceAmbientOcclusion {
                    quality_level: bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::Medium,
                    ..default()
                });
            }
            SsaoQuality::High => {
                camera.insert(bevy::pbr::ScreenSpaceAmbientOcclusion {
                    quality_level: bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::High,
                    ..default()
                });
            }
            SsaoQuality::Ultra => {
                camera.insert(bevy::pbr::ScreenSpaceAmbientOcclusion {
                    quality_level: bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::Ultra,
                    ..default()
                });
            }
        }
    }

    if settings.cinematic_mode {
        camera.remove::<OutlineSettings>();
    } else {
        camera.insert(OutlineSettings {
            threshold: 0.10,
            color: Vec4::new(
                settings.outline_red.clamp(0.0, 1.0),
                settings.outline_green.clamp(0.0, 1.0),
                settings.outline_blue.clamp(0.0, 1.0),
                settings.outline_opacity.clamp(0.0, 1.0),
            ),
        });
    }

    camera.insert(ColorGrading::with_identical_sections(
        ColorGradingGlobal {
            post_saturation: settings.saturation.clamp(0.0, 2.0),
            ..default()
        },
        ColorGradingSection {
            contrast: settings.contrast.clamp(0.5, 1.5),
            gamma: settings.gamma.clamp(0.5, 2.0),
            ..default()
        },
    ));
}

fn shadow_map_size(shadow_quality: &ShadowQuality) -> usize {
    match shadow_quality {
        ShadowQuality::Off => 1024,
        ShadowQuality::Low => 1024,
        ShadowQuality::Medium => 2048,
        ShadowQuality::High => 4096,
    }
}

pub fn apply_fps_cap(settings: Option<Res<Settings>>, mut last_frame_end: Local<Option<Instant>>) {
    let Some(settings) = settings else {
        return;
    };

    let fps_cap = settings.fps_cap;
    let now = Instant::now();
    if let Some(last_frame_end) = *last_frame_end
        && fps_cap > 0
    {
        let target_frame_time = Duration::from_secs_f64(1.0 / fps_cap as f64);
        let elapsed = now.saturating_duration_since(last_frame_end);
        if elapsed < target_frame_time {
            std::thread::sleep(target_frame_time - elapsed);
        }
    }
    *last_frame_end = Some(Instant::now());
}

pub fn sync_active_keybindings(settings: Res<Settings>, mut active: ResMut<ActiveBindings>) {
    active.sync_from(&settings.keybindings, &settings.gamepad_bindings);
}
