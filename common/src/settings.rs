use std::io::{Read, Write};

use bevy::prelude::*;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};

use crate::pawn::pawn::MouseSensitivity;
use crate::steam::SteamClient;

const SETTINGS_FILE: &str = "settings.toml";

// ── Data ─────────────────────────────────────────────────────────────────────

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

// to sync settings file, we'll just use Auto-Cloud, Cloud Sync or whatever it's called

fn load_settings(mut commands: Commands, steam: Option<Res<SteamClient>>) {
    let settings = match steam {
        // Some(steam) => load_from_steam(&steam.0),
        None => Settings::default(),
    };
    commands.insert_resource(settings);
}

fn save_settings_on_change(
    settings: Res<Settings>,
    mut sensitivity: ResMut<MouseSensitivity>,
    steam: Option<Res<SteamClient>>,
) {
    if settings.is_changed() {
        sensitivity.0 = settings.mouse_sensitivity;
        if !settings.is_added() {
            if let Some(steam) = steam {
                save_to_steam(&steam.0, &settings);
            }
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
            .add_systems(PostUpdate, save_settings_on_change);
    }
}
