use bevy_egui::egui;
use common::PromptDeviceMode;

use crate::settings::Settings;

pub fn show(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Keyboard / Mouse");
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

    ui.separator();
    ui.heading("Gamepad");

    ui.horizontal(|ui| {
        ui.label("Look sensitivity");
        ui.add(
            egui::Slider::new(&mut settings.gamepad_look_sensitivity, 0.5..=8.0).fixed_decimals(2),
        );
    });

    ui.horizontal(|ui| {
        ui.label("Move deadzone");
        ui.add(egui::Slider::new(&mut settings.gamepad_move_deadzone, 0.0..=0.5).fixed_decimals(2));
    });

    ui.horizontal(|ui| {
        ui.label("Look deadzone");
        ui.add(egui::Slider::new(&mut settings.gamepad_look_deadzone, 0.0..=0.5).fixed_decimals(2));
    });

    ui.checkbox(&mut settings.gamepad_invert_y, "Invert gamepad Y");

    ui.separator();
    ui.heading("Prompt labels");
    ui.horizontal(|ui| {
        ui.label("Show prompts as");
        ui.selectable_value(
            &mut settings.prompt_device_mode,
            PromptDeviceMode::KeyboardMouse,
            "Keyboard / mouse",
        );
        ui.selectable_value(
            &mut settings.prompt_device_mode,
            PromptDeviceMode::Gamepad,
            "Gamepad",
        );
        ui.selectable_value(
            &mut settings.prompt_device_mode,
            PromptDeviceMode::Both,
            "Both",
        );
    });
}
