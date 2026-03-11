use bevy::{
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, /* WindowMode*/ WindowResolution},
};
use bevy_egui::input::EguiWantsInput;
use crate::{GameState, UiState};

pub struct WindowSettingsPlugin;

impl Plugin for WindowSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, init_window);
        app.add_systems(Update, (toggle_ui_state, sync_cursor_lock).chain());
    }
}

fn init_window(mut window_query: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = window_query.single_mut() {
        window.resolution = WindowResolution::new(1920, 1080);
        // window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    }
}

/// Handles Escape (open/close pause menu) and click (resume).
fn toggle_ui_state(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    game_state: Res<State<GameState>>,
    ui_state: Res<State<UiState>>,
    mut next_ui: ResMut<NextState<UiState>>,
) {
    let in_game = matches!(game_state.get(), GameState::SinglePlayer | GameState::Multiplayer);
    if !in_game { return; }

    if keys.just_pressed(KeyCode::Escape) {
        match ui_state.get() {
            UiState::Playing => next_ui.set(UiState::Paused),
            _ => next_ui.set(UiState::Playing),
        }
    }

    let egui_active = egui_wants_input.map_or(false, |e| e.wants_any_input());
    if mouse.just_pressed(MouseButton::Left) && !egui_active {
        next_ui.set(UiState::Playing);
    }
}

/// Locks or unlocks the cursor based on the current game and UI state.
fn sync_cursor_lock(
    game_state: Res<State<GameState>>,
    ui_state: Res<State<UiState>>,
    mut cursor_options: Single<&mut CursorOptions>,
) {
    let should_lock = matches!(game_state.get(), GameState::SinglePlayer | GameState::Multiplayer)
        && *ui_state.get() == UiState::Playing;
    cursor_options.visible = !should_lock;
    cursor_options.grab_mode = if should_lock { CursorGrabMode::Locked } else { CursorGrabMode::None };
}
