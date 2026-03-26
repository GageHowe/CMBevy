use std::fs;
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use bevy_egui::egui;
use game_objects::pawn::{CameraEffector, MouseSensitivity};
use physics::physics_world::PhysicsInterpMode;
use serde::{Deserialize, Serialize};

// to sync settings file, we'll just use steam Auto-Cloud, Cloud Sync or whatever it's called
const SETTINGS_FILE: &str = "settings.toml";

pub struct SettingsPlugin;
impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Settings>()
            .add_systems(Startup, load_settings)
            .add_systems(PostUpdate, apply_settings.run_if(resource_changed::<Settings>))
            .add_systems(PostUpdate, save_settings.run_if(resource_changed::<Settings>));
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
    settings: Res<Settings>,
    mut sensitivity: ResMut<MouseSensitivity>,
    mut interp_mode: ResMut<PhysicsInterpMode>,
    mut cam_effects: Query<&mut CameraEffector, With<Camera3d>>,
    mut window_q: Query<&mut Window, With<PrimaryWindow>>,
) {
    sensitivity.base = settings.mouse_sensitivity;
    sensitivity.zoom_blend = settings.zoom_sensitivity_blend;

    if let Ok(mut fx) = cam_effects.single_mut() {
        fx.base_fov = settings.fov;
        fx.current_fov = settings.fov;
    }

    *interp_mode = match settings.physics_interp {
        PhysicsInterp::Off => PhysicsInterpMode::Off,
        PhysicsInterp::Interpolate => PhysicsInterpMode::Interpolate,
        PhysicsInterp::Extrapolate => PhysicsInterpMode::Extrapolate,
        PhysicsInterp::RotationOnly => PhysicsInterpMode::RotationOnly,
    };

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

pub fn show_settings_ui(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    section: &mut SettingsSection,
) {
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

    ui.horizontal(|ui| {
        ui.label("Field of view")
            .on_hover_text("Horizontal field of view in degrees. Higher values show more of the scene but increase distortion.");
        ui.add(egui::Slider::new(&mut settings.fov, 60.0..=120.0).suffix("°"));
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

fn show_input_settings(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label("Mouse sensitivity")
            .on_hover_text("How far the camera rotates per pixel of mouse movement.");
        ui.add(
            egui::Slider::new(&mut settings.mouse_sensitivity, 0.0001..=0.01)
                .logarithmic(true)
                .fixed_decimals(4),
        );
    });

    ui.horizontal(|ui| {
        ui.label("Zoom sensitivity")
            .on_hover_text("Blends between normal mouse sensitivity and full zoom slowdown while scoped.");
        ui.add(
            egui::Slider::new(&mut settings.zoom_sensitivity_blend, 0.0..=1.0)
                .fixed_decimals(2)
                .show_value(true),
        );
    });
}
