mod audio;
mod graphics;
mod input;
mod misc;

use bevy::prelude::*;
use bevy_egui::egui;

use super::{
    controls::{ControlsCapture, show_controls_settings},
    data::{Settings, SettingsSection},
    persistence,
};
use crate::sound::AudioOutputDevices;

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
        ui.selectable_value(section, SettingsSection::Misc, "Misc");
    });
    ui.separator();

    match section {
        SettingsSection::Graphics => graphics::show(ui, settings),
        SettingsSection::Audio => audio::show(ui, settings, audio_outputs),
        SettingsSection::Input => input::show(ui, settings),
        SettingsSection::Controls => show_controls_settings(ui, settings, keyboard, mouse, capture),
        SettingsSection::Misc => misc::show(ui, settings),
    }

    ui.separator();
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
