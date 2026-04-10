mod audio;
mod graphics;
mod input;

use bevy::prelude::*;
use bevy_egui::egui;

use crate::sound::AudioOutputDevices;

use super::controls::{ControlsCapture, show_controls_settings};
use super::data::{Settings, SettingsSection};
use super::persistence;

pub fn show_settings_ui(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    section: &mut SettingsSection,
    audio_outputs: &AudioOutputDevices,
    keyboard: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    capture: &mut ControlsCapture,
) {
    ui.horizontal(|ui| {
        ui.selectable_value(section, SettingsSection::Graphics, "Graphics");
        ui.selectable_value(section, SettingsSection::Audio, "Audio");
        ui.selectable_value(section, SettingsSection::Input, "Input");
        ui.selectable_value(section, SettingsSection::Controls, "Controls");
    });
    ui.separator();

    match section {
        SettingsSection::Graphics => graphics::show(ui, settings),
        SettingsSection::Audio => audio::show(ui, settings, audio_outputs),
        SettingsSection::Input => input::show(ui, settings),
        SettingsSection::Controls => {
            show_controls_settings(ui, settings, keyboard, mouse, capture)
        }
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("View settings file").clicked()
            && let Err(err) = persistence::reveal_settings_file()
        {
            warn!("failed to open settings file location: {err}");
        }
        if ui.button("Reset all settings").clicked() {
            capture.cancel();
            persistence::reset_settings(settings);
        }
    });
}
