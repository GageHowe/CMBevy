use bevy_egui::egui;

use crate::settings::{PhysicsSubsteps, Settings};

pub fn show(ui: &mut egui::Ui, settings: &mut Settings) {
    egui::CollapsingHeader::new("Simulation")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Physics substeps")
                    .on_hover_text("Run multiple physics steps per tick. Improves accuracy at the cost of CPU time. Ignored during reconciliation.");
                ui.selectable_value(&mut settings.physics_substeps, PhysicsSubsteps::One, "Off")
                    .on_hover_text("One physics step per tick.");
                ui.selectable_value(&mut settings.physics_substeps, PhysicsSubsteps::Two, "2x")
                    .on_hover_text("Two physics steps per tick.");
                ui.selectable_value(&mut settings.physics_substeps, PhysicsSubsteps::Four, "4x")
                    .on_hover_text("Four physics steps per tick.");
            });
        });

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
