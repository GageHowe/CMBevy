use std::path::PathBuf;
use bevy::prelude::*;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};
use std::fs;
// use serde::

use common::pawn::pawn::MouseSensitivity;

const SETTINGS_FILE: &str = "settings.toml";

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
pub enum PhysicsInterp {
    Off,
    Interpolate,
    #[default]
    Extrapolate,
}

#[derive(Resource, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Resource)]
pub struct Settings {
    pub mouse_sensitivity: f32,
    pub fov: f32,
    pub physics_interp: PhysicsInterp,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.002,
            fov: 90.0,
            physics_interp: PhysicsInterp::Extrapolate,
        }
    }
}

// to sync settings file, we'll just use steam Auto-Cloud, Cloud Sync or whatever it's called

fn load_settings(mut commands: Commands) {
    // todo: change this to the app install directory
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

fn change_settings(
    settings: Res<Settings>,
    mut sensitivity: ResMut<MouseSensitivity>,
    mut projection: Query<&mut Projection, With<Camera3d>>,
) {
    if settings.is_changed() {
        sensitivity.0 = settings.mouse_sensitivity;
        if let Ok(mut proj) = projection.single_mut() {
            if let Projection::Perspective(ref mut p) = *proj {
                p.fov = settings.fov.to_radians();
            }
        }
        if !settings.is_added() {
            save_settings(&settings);
        }
    }
}

fn save_settings(settings: &Settings) {
    let path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CMBevy")
        .join(SETTINGS_FILE);
    if let Ok(s) = toml::to_string_pretty(settings) {
        fs::write(path, s).ok();
    }
}

// ── UI ────────────────────────────────────────────────────────────────────────

/// Call this inside any egui window/panel to render the settings controls.
pub fn show_settings_ui(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Settings");
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Mouse sensitivity");
        ui.add(
            egui::Slider::new(&mut settings.mouse_sensitivity, 0.0001..=0.01)
                .logarithmic(true)
                .fixed_decimals(4),
        );
    });

    ui.horizontal(|ui| {
        ui.label("Field of view");
        ui.add(egui::Slider::new(&mut settings.fov, 60.0..=120.0).suffix("°"));
    });

    ui.horizontal(|ui| {
        ui.label("Physics interpolation");
        ui.selectable_value(&mut settings.physics_interp, PhysicsInterp::Off, "Off");
        ui.selectable_value(&mut settings.physics_interp, PhysicsInterp::Interpolate, "Interpolate");
        ui.selectable_value(&mut settings.physics_interp, PhysicsInterp::Extrapolate, "Extrapolate");
    });
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Settings>()
            .add_systems(Startup, load_settings)
            .add_systems(PostUpdate, change_settings);
    }
}
