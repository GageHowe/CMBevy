use ::audio::AudioOutputDevices;
use bevy_egui::egui;

use crate::settings::Settings;

pub fn show(ui: &mut egui::Ui, settings: &mut Settings, audio_outputs: &AudioOutputDevices) {
    egui::CollapsingHeader::new("Output")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Output device").on_hover_text(
                    "Selects the FMOD output device. System Default uses the device FMOD picked at startup.",
                );
                let selected = if settings.audio_output_device.is_empty() {
                    "System Default"
                } else {
                    settings.audio_output_device.as_str()
                };
                egui::ComboBox::from_id_salt("audio_output_device_combo")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut settings.audio_output_device,
                            String::new(),
                            "System Default",
                        );
                        for name in &audio_outputs.names {
                            ui.selectable_value(
                                &mut settings.audio_output_device,
                                name.clone(),
                                name,
                            );
                        }
                    });
            });
        });

    egui::CollapsingHeader::new("Latency")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("FMOD buffer")
                    .on_hover_text("FMOD DSP buffer size in samples. Lower is more responsive but more prone to crackle. Takes effect on restart.");
                egui::ComboBox::from_id_salt("fmod_buffer_size_combo")
                    .selected_text(settings.fmod_buffer_size.to_string())
                    .show_ui(ui, |ui| {
                        for size in [128_u32, 256, 512, 1024] {
                            ui.selectable_value(
                                &mut settings.fmod_buffer_size,
                                size,
                                size.to_string(),
                            );
                        }
                    });
            });
        });
}
