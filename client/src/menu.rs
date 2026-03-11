use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use crate::{GameState, UiState};
use crate::settings::{show_settings_ui, Settings};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, main_menu.run_if(in_state(GameState::MainMenu)));
        app.add_systems(EguiPrimaryContextPass, pause_menu.run_if(in_state(UiState::Paused)));
        app.add_systems(EguiPrimaryContextPass, settings_menu.run_if(in_state(UiState::Settings)));
    }
}

fn main_menu(mut contexts: EguiContexts, mut next_state: ResMut<NextState<GameState>>) {
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

fn pause_menu(
    mut contexts: EguiContexts,
    mut next_game: ResMut<NextState<GameState>>,
    mut next_ui: ResMut<NextState<UiState>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let center = ctx.content_rect().center();
    egui::Window::new("Paused")
        .default_pos(center)
        .pivot(egui::Align2::CENTER_CENTER)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.set_min_width(200.0);
            ui.vertical_centered(|ui| {
                if ui.button("Resume").clicked() {
                    next_ui.set(UiState::Playing);
                }
                ui.add_space(4.0);
                if ui.button("Settings").clicked() {
                    next_ui.set(UiState::Settings);
                }
                ui.add_space(4.0);
                if ui.button("Quit to Menu").clicked() {
                    next_game.set(GameState::MainMenu);
                    next_ui.set(UiState::Playing);
                }
            });
        });
}

fn settings_menu(
    mut contexts: EguiContexts,
    mut next_ui: ResMut<NextState<UiState>>,
    mut settings: ResMut<Settings>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let center = ctx.content_rect().center();
    egui::Window::new("Settings")
        .default_pos(center)
        .pivot(egui::Align2::CENTER_CENTER)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.set_min_width(250.0);
            show_settings_ui(ui, &mut settings);
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if ui.button("Back").clicked() {
                    next_ui.set(UiState::Paused);
                }
            });
        });
}
