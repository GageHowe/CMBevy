use bevy_egui::egui;

use crate::settings::Settings;

pub fn show(ui: &mut egui::Ui, settings: &mut Settings) {
    egui::CollapsingHeader::new("Debug")
        .default_open(true)
        .show(ui, |ui| {
            ui.checkbox(&mut settings.debug_panel, "Debug panel")
                .on_hover_text(
                    "Shows the top-left engineering overlay with rigidbody count, FPS, RTT, and quit button.",
                );
            ui.checkbox(&mut settings.cinematic_mode, "Cinematic mode")
                .on_hover_text(
                    "Hides gameplay helper overlays like planet radii during normal play.",
                );
            ui.checkbox(&mut settings.debug_render, "Debug rendering")
                .on_hover_text(
                    "Draws engineering/debug visuals like collider, seat, and projectile gizmos.",
                );
        });
}
