use std::time::{Duration, Instant};

use crate::outline::OutlineSettings;
use bevy::core_pipeline::prepass::MotionVectorPrepass;
use bevy::prelude::*;
use bevy::post_process::motion_blur::MotionBlur;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use bevy_egui::{EguiContextSettings, PrimaryEguiContext};
use common::ActiveKeyBindings;
use game_objects::pawn::{CameraEffector, LookSnapCompensation, MouseSensitivity};
use physics::physics_world::PhysicsInterpMode;

use super::data::{DisplayMode, PhysicsInterp, Settings, ShadowQuality, SsaoQuality, VsyncMode};

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
    sensitivity.base = settings.mouse_sensitivity;
    sensitivity.zoom_blend = settings.zoom_sensitivity_blend;
    sensitivity.vehicle_pitch_yaw = settings.vehicle_pitch_yaw_sensitivity;
    snap_comp.0 = settings.preserve_look_across_planet_snap;

    let mut window_size = None;
    if let Ok(mut window) = window_q.single_mut() {
        window_size = Some(window.physical_size());
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
        apply_camera_graphics(&mut camera, &settings, window_size);
    }

    *interp_mode = match settings.physics_interp {
        PhysicsInterp::Off => PhysicsInterpMode::Off,
        PhysicsInterp::Interpolate => PhysicsInterpMode::Interpolate,
        PhysicsInterp::Extrapolate => PhysicsInterpMode::Extrapolate,
        PhysicsInterp::RotationOnly => PhysicsInterpMode::RotationOnly,
    };

    if let Ok(mut egui_settings) = egui_context_settings.single_mut() {
        egui_settings.scale_factor = settings.ui_scale;
    }

    directional_light_shadow_map.size = shadow_map_size(&settings.shadow_quality);
    for mut directional_light in &mut directional_lights {
        directional_light.shadows_enabled = !matches!(settings.shadow_quality, ShadowQuality::Off);
    }
}

pub fn sync_dynamic_graphics_settings(
    mut commands: Commands,
    settings: Option<Res<Settings>>,
    primary_window_q: Query<&Window, With<PrimaryWindow>>,
    resized_window_q: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    camera_entities: Query<Entity, With<Camera3d>>,
    added_cameras: Query<Entity, Added<Camera3d>>,
    mut added_directional_lights: Query<&mut DirectionalLight, Added<DirectionalLight>>,
    mut directional_light_shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
) {
    let Some(settings) = settings else {
        return;
    };

    let window_size = primary_window_q.single().ok().map(Window::physical_size);
    let window_changed = resized_window_q.single().is_ok();

    for camera_entity in &added_cameras {
        let mut camera = commands.entity(camera_entity);
        apply_camera_graphics(&mut camera, &settings, window_size);
    }

    if window_changed {
        for camera_entity in &camera_entities {
            let mut camera = commands.entity(camera_entity);
            apply_render_scale(&mut camera, &settings, window_size);
        }
    }

    if !added_directional_lights.is_empty() {
        directional_light_shadow_map.size = shadow_map_size(&settings.shadow_quality);
        for mut directional_light in &mut added_directional_lights {
            directional_light.shadows_enabled =
                !matches!(settings.shadow_quality, ShadowQuality::Off);
        }
    }
}

fn apply_camera_graphics(
    camera: &mut EntityCommands,
    settings: &Settings,
    window_size: Option<UVec2>,
) {
    if settings.anti_aliasing {
        camera.insert(bevy::anti_alias::smaa::Smaa::default());
    } else {
        camera.remove::<bevy::anti_alias::smaa::Smaa>();
    }

    if settings.auto_exposure {
        camera.insert(bevy::post_process::auto_exposure::AutoExposure {
            range: -12.0..=4.0,
            speed_brighten: 0.2,
            speed_darken: 0.1,
            filter: 0.0..=0.50,
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
            shutter_angle: 0.5,
            samples: 1,
        });
    } else {
        camera.remove::<MotionBlur>();
        camera.remove::<MotionVectorPrepass>();
    }

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

    if settings.cinematic_mode {
        camera.remove::<OutlineSettings>();
    } else {
        camera.insert(OutlineSettings {
            threshold: 0.10,
            color: Vec4::new(0.5, 0.5, 0.5, 0.03),
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

    apply_render_scale(camera, settings, window_size);
}

fn apply_render_scale(
    camera: &mut EntityCommands,
    settings: &Settings,
    window_size: Option<UVec2>,
) {
    let render_scale = settings.render_scale.clamp(0.25, 1.0);
    if render_scale >= 0.99 {
        camera.remove::<bevy::camera::MainPassResolutionOverride>();
        return;
    }

    let Some(window_size) = window_size else {
        return;
    };

    let scaled = window_size.as_vec2() * render_scale;
    let scaled = scaled.max(Vec2::splat(1.0)).round().as_uvec2();
    camera.insert(bevy::camera::MainPassResolutionOverride(scaled));
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

pub fn sync_active_keybindings(settings: Res<Settings>, mut active: ResMut<ActiveKeyBindings>) {
    active.sync_from(&settings.keybindings);
}
