use std::path::PathBuf;
use bevy::prelude::*;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};
use std::fs;
// use serde::

use crate::pawn::pawn::MouseSensitivity;

const SETTINGS_FILE: &str = "settings.toml";

#[derive(Resource, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Resource)]
pub struct Settings {
    pub mouse_sensitivity: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.002,
        }
    }
}

// to sync settings file, we'll just use steam Auto-Cloud, Cloud Sync or whatever it's called

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

/// TODO: split this up into separate save functions for efficiency, if possible
fn change_settings (
    settings: Res<Settings>,
    mut sensitivity: ResMut<MouseSensitivity>,
) {
    if settings.is_changed() {
        sensitivity.0 = settings.mouse_sensitivity;
        if !settings.is_added() {
            // if let Some(steam) = steam {
            //     save_to_steam(&steam.0, &settings);
            // }
        }
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
