use bevy_egui::egui;

use crate::settings::Settings;

pub fn show(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label("Mouse sensitivity").on_hover_text("You know what this does.");
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
