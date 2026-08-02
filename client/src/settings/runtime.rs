use std::time::{Duration, Instant};

use bevy::{
    core_pipeline::prepass::MotionVectorPrepass,
    pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel},
    post_process::{
        effect_stack::{LensDistortion, Vignette},
        motion_blur::MotionBlur,
    },
    prelude::*,
    render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode},
};
use bevy_egui::{EguiContext, PrimaryEguiContext};
use common::{ActiveBindings, PromptDevicePreference};
use gameplay::pawn::{CameraEffector, MouseSensitivity};
use physics::physics_world::PhysicsInterpMode;

use super::data::{DisplayMode, PhysicsInterp, Settings, ShadowQuality, SsaoQuality, VsyncMode};
use crate::outline::OutlineSettings;

macro_rules! impl_same_name_from {
    ($src:ident => $dst:ty { $($variant:ident),* $(,)? }) => {
        impl From<$src> for $dst {
            fn from(value: $src) -> Self {
                match value {
                    $($src::$variant => Self::$variant,)*
                }
            }
        }
    };
}

impl_same_name_from!(VsyncMode => PresentMode {
    AutoVsync,
    AutoNoVsync,
    Fifo,
    FifoRelaxed,
    Immediate,
    Mailbox,
});

impl From<DisplayMode> for WindowMode {
    fn from(value: DisplayMode) -> Self {
        match value {
            DisplayMode::Windowed => Self::Windowed,
            DisplayMode::BorderlessFullscreen => {
                Self::BorderlessFullscreen(MonitorSelection::Current)
            }
        }
    }
}

impl_same_name_from!(PhysicsInterp => PhysicsInterpMode {
    Off,
    Interpolate,
    Extrapolate,
    Balanced,
});

impl From<SsaoQuality> for Option<ScreenSpaceAmbientOcclusionQualityLevel> {
    fn from(value: SsaoQuality) -> Self {
        match value {
            SsaoQuality::Off => None,
            SsaoQuality::Medium => Some(ScreenSpaceAmbientOcclusionQualityLevel::Medium),
            SsaoQuality::High => Some(ScreenSpaceAmbientOcclusionQualityLevel::High),
            SsaoQuality::Ultra => Some(ScreenSpaceAmbientOcclusionQualityLevel::Ultra),
        }
    }
}

impl From<ShadowQuality> for usize {
    fn from(value: ShadowQuality) -> Self {
        match value {
            ShadowQuality::Off | ShadowQuality::Low => 1024,
            ShadowQuality::Medium => 2048,
            ShadowQuality::High => 4096,
        }
    }
}

pub fn apply_settings(
    mut commands: Commands,
    settings: Res<Settings>,
    mut sensitivity: ResMut<MouseSensitivity>,
    mut interp_mode: ResMut<PhysicsInterpMode>,
    mut cam_effects: Query<(Entity, &mut CameraEffector), With<Camera3d>>,
    mut window_q: Query<&mut Window, With<PrimaryWindow>>,
    mut directional_lights: Query<&mut DirectionalLight>,
    mut directional_light_shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
    mut egui_context: Query<&mut EguiContext, With<PrimaryEguiContext>>,
) {
    commands.insert_resource(PromptDevicePreference(settings.prompt_device_mode));
    sensitivity.base = settings.mouse_sensitivity;
    sensitivity.zoom_blend = settings.zoom_sensitivity_blend;
    sensitivity.vehicle_pitch_yaw = settings.vehicle_pitch_yaw_sensitivity;
    sensitivity.gamepad_look = settings.gamepad_look_sensitivity;
    sensitivity.gamepad_move_deadzone = settings.gamepad_move_deadzone;
    sensitivity.gamepad_look_deadzone = settings.gamepad_look_deadzone;
    sensitivity.gamepad_invert_y = settings.gamepad_invert_y;

    if let Ok(mut window) = window_q.single_mut() {
        window.present_mode = settings.vsync.into();
        window.mode = settings.display_mode.into();
    }

    if let Ok((camera_entity, mut fx)) = cam_effects.single_mut() {
        fx.base_fov = settings.fov;
        fx.current_fov = settings.fov;

        let mut camera = commands.entity(camera_entity);
        apply_camera_graphics(&mut camera, &settings);
    }

    *interp_mode = settings.physics_interp.into();
    if let Ok(mut egui_context) = egui_context.single_mut() {
        egui_context.get_mut().set_zoom_factor(settings.ui_scale);
    }

    directional_light_shadow_map.size = settings.shadow_quality.into();
    for mut directional_light in &mut directional_lights {
        apply_directional_light_shadows(&mut directional_light, &settings.shadow_quality);
    }
}

pub fn sync_dynamic_graphics_settings(
    mut commands: Commands,
    settings: Option<Res<Settings>>,
    added_cameras: Query<Entity, Added<Camera3d>>,
    mut added_directional_lights: Query<&mut DirectionalLight, Added<DirectionalLight>>,
    _directional_light_shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
) {
    let Some(settings) = settings else {
        return;
    };

    for camera_entity in &added_cameras {
        let mut camera = commands.entity(camera_entity);
        apply_camera_graphics(&mut camera, &settings);
    }
    for mut light in &mut added_directional_lights {
        apply_directional_light_shadows(&mut light, &settings.shadow_quality);
    }
}

fn apply_camera_graphics(camera: &mut EntityCommands, settings: &Settings) {
    if settings.anti_aliasing {
        camera.insert(bevy::anti_alias::smaa::Smaa::default());
    } else {
        camera.remove::<bevy::anti_alias::smaa::Smaa>();
    }

    if settings.auto_exposure {
        camera.insert(bevy::post_process::auto_exposure::AutoExposure {
            range: -3.0..=0.0,
            // speed_brighten: 1.5,
            filter: 0.1..=0.9,
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

    if settings.vignette {
        camera.insert(Vignette {
            intensity: settings.vignette_intensity.clamp(0.0, 1.0),
            radius: 0.85,
            smoothness: 1.2,
            ..default()
        });
    } else {
        camera.remove::<Vignette>();
    }

    if settings.lens_distortion {
        let intensity = settings.lens_distortion_intensity.clamp(-0.25, 0.25);
        camera.insert(LensDistortion {
            intensity,
            scale: 1.0 + intensity.abs() * 1.5,
            ..default()
        });
    } else {
        camera.remove::<LensDistortion>();
    }

    if let Some(quality_level) = settings.ssao_quality.into() {
        camera.insert(ScreenSpaceAmbientOcclusion {
            quality_level,
            ..default()
        });
    } else {
        camera.remove::<ScreenSpaceAmbientOcclusion>();
    }

    if matches!(settings.shadow_quality, ShadowQuality::Off) {
        camera.remove::<bevy::pbr::ContactShadows>();
    } else {
        camera.insert(bevy::pbr::ContactShadows::default());
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

fn apply_directional_light_shadows(light: &mut DirectionalLight, shadow_quality: &ShadowQuality) {
    let enabled = !matches!(shadow_quality, ShadowQuality::Off);
    light.shadow_maps_enabled = enabled;
    light.contact_shadows_enabled = enabled;
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
