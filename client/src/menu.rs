use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use crate::GameState;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, menu_ui.run_if(in_state(GameState::MainMenu)));
    }
}

fn menu_ui(mut contexts: EguiContexts, mut next_state: ResMut<NextState<GameState>>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let center = ctx.content_rect().center();
    egui::Window::new("Critical Mass")
        .default_pos(center)
        .pivot(egui::Align2::CENTER_CENTER)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.set_min_width(200.0);
            ui.vertical_centered(|ui| {
                if ui.button("Singleplayer").clicked() {
                    next_state.set(GameState::SinglePlayer);
                }
                ui.add_space(4.0);
                if ui.button("Multiplayer").clicked() {
                    next_state.set(GameState::Multiplayer);
                }
            });
        });
}
