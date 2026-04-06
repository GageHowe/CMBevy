use std::fs;
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use bevy_egui::{EguiContextSettings, PrimaryEguiContext, egui};
use game_objects::pawn::{CameraEffector, LookSnapCompensation, MouseSensitivity};
use physics::physics_world::PhysicsInterpMode;
use serde::{Deserialize, Serialize};

// to sync settings file, we'll just use steam Auto-Cloud, Cloud Sync or whatever it's called
const SETTINGS_FILE: &str = "settings.toml";

pub struct SettingsPlugin;
impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Settings>()
            .add_systems(Startup, load_settings)
            .add_systems(
                PostUpdate,
                apply_settings.run_if(resource_changed::<Settings>),
            )
            .add_systems(
                PostUpdate,
                save_settings.run_if(resource_changed::<Settings>),
            );
    }
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
pub enum PhysicsInterp {
    Off,
    Interpolate,
    Extrapolate,
    #[default]
    RotationOnly,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
pub enum VsyncMode {
    AutoVsync,
    AutoNoVsync,
    Fifo,
    #[default]
    FifoRelaxed,
    Immediate,
    Mailbox,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
pub enum DisplayMode {
    #[default]
    Windowed,
    BorderlessFullscreen,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
pub enum SsaoQuality {
    #[default]
    Off,
    Medium,
    High,
    Ultra,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    Graphics,
    Input,
    Controls,
}

#[derive(Resource, Serialize, Deserialize, Clone, Reflect)]
#[serde(default)]
#[reflect(Resource)]
pub struct Settings {
    pub mouse_sensitivity: f32,
    pub zoom_sensitivity_blend: f32,
    pub vehicle_pitch_yaw_sensitivity: f32,
    pub preserve_look_across_planet_snap: bool,
    pub ui_scale: f32,
    pub anti_aliasing: bool,
    pub auto_exposure: bool,
    pub bloom: bool,
    pub ssao_quality: SsaoQuality,
    pub fov: f32,
    pub physics_interp: PhysicsInterp,
    pub debug_render: bool,
    pub vsync: VsyncMode,
    pub display_mode: DisplayMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.002,
            zoom_sensitivity_blend: 1.0,
            vehicle_pitch_yaw_sensitivity: 0.002,
            preserve_look_across_planet_snap: false,
            ui_scale: 1.5,
            anti_aliasing: true,
            auto_exposure: true,
            bloom: true,
            ssao_quality: SsaoQuality::Medium,
            fov: 90.0,
            physics_interp: PhysicsInterp::RotationOnly,
            debug_render: false,
            vsync: VsyncMode::FifoRelaxed,
            display_mode: DisplayMode::Windowed,
        }
    }
}

fn load_settings(mut commands: Commands) {
    let path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CMBevy")
        .join(SETTINGS_FILE);

    let settings = if path.exists() {
        let contents = fs::read_to_string(&path).unwrap_or_default();
        toml::from_str(&contents).unwrap_or_default()
    } else {
        let default = Settings::default();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, toml::to_string_pretty(&default).unwrap_or_default()).ok();
        default
    };

    commands.insert_resource(settings);
}

fn apply_settings(
    mut commands: Commands,
    settings: Res<Settings>,
    mut sensitivity: ResMut<MouseSensitivity>,
    mut snap_comp: ResMut<LookSnapCompensation>,
    mut interp_mode: ResMut<PhysicsInterpMode>,
    mut cam_effects: Query<(Entity, &mut CameraEffector), With<Camera3d>>,
    mut window_q: Query<&mut Window, With<PrimaryWindow>>,
    mut egui_context_settings: Query<&mut EguiContextSettings, With<PrimaryEguiContext>>,
) {
    sensitivity.base = settings.mouse_sensitivity;
    sensitivity.zoom_blend = settings.zoom_sensitivity_blend;
    sensitivity.vehicle_pitch_yaw = settings.vehicle_pitch_yaw_sensitivity;
    snap_comp.0 = settings.preserve_look_across_planet_snap;

    if let Ok((camera_entity, mut fx)) = cam_effects.single_mut() {
        fx.base_fov = settings.fov;
        fx.current_fov = settings.fov;

        let mut camera = commands.entity(camera_entity);
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
                intensity: 0.4,
                composite_mode: bevy::post_process::bloom::BloomCompositeMode::Additive,
                high_pass_frequency: 0.5,
                ..default()
            });
        } else {
            camera.remove::<bevy::post_process::bloom::Bloom>();
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
    }

    *interp_mode = match settings.physics_interp {
        PhysicsInterp::Off => PhysicsInterpMode::Off,
        PhysicsInterp::Interpolate => PhysicsInterpMode::Interpolate,
        PhysicsInterp::Extrapolate => PhysicsInterpMode::Extrapolate,
        PhysicsInterp::RotationOnly => PhysicsInterpMode::RotationOnly,
    };

    if let Ok(mut egui_settings) = egui_context_settings.single_mut() {
        // Keep UI scaling global so every egui surface stays in sync with the saved setting.
        egui_settings.scale_factor = settings.ui_scale;
    }

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
            // Borderless fullscreen is the safer fullscreen mode on desktop and
            // avoids the OS-hostile behavior we were seeing with exclusive fullscreen.
            DisplayMode::BorderlessFullscreen => {
                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
            }
        };
    }
}

fn save_settings(settings: Res<Settings>) {
    if settings.is_added() {
        return;
    }
    let path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CMBevy")
        .join(SETTINGS_FILE);
    if let Ok(serialized) = toml::to_string_pretty(&*settings) {
        fs::write(path, serialized).ok();
    }
}

pub fn show_settings_ui(ui: &mut egui::Ui, settings: &mut Settings, section: &mut SettingsSection) {
    ui.horizontal(|ui| {
        ui.selectable_value(section, SettingsSection::Graphics, "Graphics");
        ui.selectable_value(section, SettingsSection::Input, "Input");
        ui.selectable_value(section, SettingsSection::Controls, "Controls");
    });
    ui.separator();

    match section {
        SettingsSection::Graphics => show_graphics_settings(ui, settings),
        SettingsSection::Input => show_input_settings(ui, settings),
        SettingsSection::Controls => {
            ui.label("Control remapping coming soon.");
        }
    }
}

fn show_graphics_settings(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label("Display mode")
            .on_hover_text("Windowed is normal desktop mode. Borderless fullscreen is fullscreen without exclusive mode.");
        egui::ComboBox::from_id_salt("display_mode_combo")
            .selected_text(match settings.display_mode {
                DisplayMode::Windowed => "Windowed",
                DisplayMode::BorderlessFullscreen => "Borderless fullscreen",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut settings.display_mode, DisplayMode::Windowed, "Windowed");
                ui.selectable_value(
                    &mut settings.display_mode,
                    DisplayMode::BorderlessFullscreen,
                    "Borderless fullscreen",
                );
            });
    });

    ui.horizontal(|ui| {
        ui.label("VSync").on_hover_text("Controls how frames are presented to the display.");
        egui::ComboBox::from_id_salt("vsync_combo")
            .selected_text(match settings.vsync {
                VsyncMode::AutoVsync => "Auto (VSync)",
                VsyncMode::AutoNoVsync => "Auto (No VSync)",
                VsyncMode::Fifo => "Fifo",
                VsyncMode::FifoRelaxed => "Fifo Relaxed",
                VsyncMode::Immediate => "Immediate",
                VsyncMode::Mailbox => "Mailbox",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut settings.vsync, VsyncMode::AutoVsync, "Auto (VSync)")
                    .on_hover_text("Picks the best available VSync mode for your platform.");
                ui.selectable_value(&mut settings.vsync, VsyncMode::AutoNoVsync, "Auto (No VSync)")
                    .on_hover_text("Picks the best available non-VSync mode for your platform.");
                ui.selectable_value(&mut settings.vsync, VsyncMode::Fifo, "Fifo")
                    .on_hover_text("Traditional VSync. Frames queue up and are shown on each vertical blank. Eliminates tearing, may increase latency.");
                ui.selectable_value(&mut settings.vsync, VsyncMode::FifoRelaxed, "Fifo Relaxed")
                    .on_hover_text("Like Fifo but shows a late frame immediately instead of waiting for the next blank. Reduces latency spikes at the cost of occasional tearing.");
                ui.selectable_value(&mut settings.vsync, VsyncMode::Immediate, "Immediate")
                    .on_hover_text("No VSync. Frames are shown as soon as they are ready. Lowest latency, but may tear.");
                ui.selectable_value(&mut settings.vsync, VsyncMode::Mailbox, "Mailbox")
                    .on_hover_text("Triple buffering. Replaces the queued frame with the newest one. Low latency with no tearing, but uses more GPU power.");
            });
    });

    ui.checkbox(&mut settings.anti_aliasing, "Anti-aliasing")
        .on_hover_text("Subpixel Morphological Anti-Aliasing (SMAA). Smoothes rough pixels.");

    ui.checkbox(&mut settings.auto_exposure, "Auto exposure")
        .on_hover_text("Automatically adapts camera exposure to brightness.");

    ui.checkbox(&mut settings.bloom, "Bloom")
        .on_hover_text("Adds glow around bright areas.");

    ui.horizontal(|ui| {
        ui.label("SSAO").on_hover_text(
            "GTAO-like screen-space ambient occlusion. Adds depth and contact shadowing.",
        );
        egui::ComboBox::from_id_salt("ssao_quality_combo")
            .selected_text(match settings.ssao_quality {
                SsaoQuality::Off => "Off",
                SsaoQuality::Medium => "Medium",
                SsaoQuality::High => "High",
                SsaoQuality::Ultra => "Ultra",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut settings.ssao_quality, SsaoQuality::Off, "Off");
                ui.selectable_value(&mut settings.ssao_quality, SsaoQuality::Medium, "Medium");
                ui.selectable_value(&mut settings.ssao_quality, SsaoQuality::High, "High");
                ui.selectable_value(&mut settings.ssao_quality, SsaoQuality::Ultra, "Ultra");
            });
    });

    ui.horizontal(|ui| {
        ui.label("Field of view")
            .on_hover_text("Horizontal field of view in degrees.");
        ui.add(egui::Slider::new(&mut settings.fov, 60.0..=120.0).suffix("°"));
    });

    ui.horizontal(|ui| {
        ui.label("UI size")
            .on_hover_text("Scales the entire interface globally.");
        show_ui_scale_input(ui, settings);
    });

    ui.horizontal(|ui| {
        ui.label("Physics interpolation")
            .on_hover_text("How visual positions are smoothed between physics ticks.");
        ui.selectable_value(&mut settings.physics_interp, PhysicsInterp::Off, "Off")
            .on_hover_text("No smoothing. Objects snap to their physics position each tick.");
        ui.selectable_value(
            &mut settings.physics_interp,
            PhysicsInterp::Interpolate,
            "Interpolate",
        )
        .on_hover_text("Blends between the previous and current physics tick. Adds one tick of visual latency.");
        ui.selectable_value(
            &mut settings.physics_interp,
            PhysicsInterp::Extrapolate,
            "Extrapolate",
        )
        .on_hover_text("Predicts ahead using current velocity. No added latency but can overshoot.");
        ui.selectable_value(
            &mut settings.physics_interp,
            PhysicsInterp::RotationOnly,
            "Rotation only",
        )
        .on_hover_text("Only smooths rotation; position is not interpolated. Good balance of responsiveness and smoothness.");
    });

    ui.checkbox(&mut settings.debug_render, "Debug rendering")
        .on_hover_text("Draws physics colliders, planet radii, and projectile paths.");
}

fn show_ui_scale_input(ui: &mut egui::Ui, settings: &mut Settings) {
    let id = ui.make_persistent_id("ui_scale_input");
    let mut text = ui
        .data_mut(|data| data.get_persisted::<String>(id))
        .unwrap_or_else(|| format!("{:.0}", settings.ui_scale * 100.0));

    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(id)
            .desired_width(56.0)
            .hint_text("150"),
    );
    ui.label("%");

    if response.changed() {
        ui.data_mut(|data| data.insert_persisted(id, text.clone()));
    }

    if response.lost_focus() {
        if let Ok(percent) = text.trim().parse::<f32>() {
            let clamped_percent = percent.clamp(50.0, 250.0);
            settings.ui_scale = clamped_percent / 100.0;
            text = format!("{clamped_percent:.0}");
        } else {
            text = format!("{:.0}", settings.ui_scale * 100.0);
        }
        ui.data_mut(|data| data.insert_persisted(id, text));
    }
}

fn show_input_settings(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label("Mouse sensitivity")
            .on_hover_text("You know what this does.");
        ui.add(
            egui::Slider::new(&mut settings.mouse_sensitivity, 0.0001..=0.01)
                .logarithmic(true)
                .fixed_decimals(4),
        );
    });

    ui.horizontal(|ui| {
        ui.label("Vehicle pitch/yaw")
            .on_hover_text("Controls how sensitive vehicles are to mouse movement.");
        ui.add(
            egui::Slider::new(&mut settings.vehicle_pitch_yaw_sensitivity, 0.0001..=0.01)
                .logarithmic(true)
                .fixed_decimals(4),
        );
    });

    ui.horizontal(|ui| {
        ui.label("Zoom sensitivity").on_hover_text(
            "Blends between normal mouse sensitivity and full zoom slowdown while scoped. At 0, scope doesn't change zoom",
        );
        ui.add(
            egui::Slider::new(&mut settings.zoom_sensitivity_blend, 0.0..=1.0)
                .fixed_decimals(2)
                .show_value(true),
        );
    });

    ui.checkbox(
        &mut settings.preserve_look_across_planet_snap,
        "Preserve look across planet snap",
    )
    .on_hover_text(
        "EXPERIMENTAL: Keeps the camera aimed in the same world direction when planet snapping rotates the player frame. This may cause camera jitter.",
    );
}
