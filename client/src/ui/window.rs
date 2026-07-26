use bevy::{
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, /* WindowMode*/ WindowResolution},
};
use bevy_egui::input::EguiWantsInput;
use common::{ActiveBindings, InputAction, active_gamepad};

use crate::{GameState, SimState, UiState, settings::ControlsCapture};

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
    active_bindings: Res<ActiveBindings>,
    gamepads: Query<&Gamepad>,
    capture: Option<Res<ControlsCapture>>,
    game_state: Res<State<GameState>>,
    ui_state: Res<State<UiState>>,
    mut next_ui: ResMut<NextState<UiState>>,
    mut next_sim: ResMut<NextState<SimState>>,
) {
    let in_game = matches!(
        game_state.get(),
        GameState::SinglePlayer | GameState::Multiplayer
    );
    if !in_game {
        return;
    }
    if capture.as_ref().is_some_and(|capture| capture.is_active()) {
        return;
    }

    let egui_wants_keyboard = egui_wants_input
        .as_ref()
        .map_or(false, |e| e.wants_keyboard_input());

    // Don't open the pause menu if the chat input has keyboard focus.
    if active_bindings.just_pressed(
        InputAction::Pause,
        &keys,
        &mouse,
        active_gamepad(gamepads.iter()),
    ) && !egui_wants_keyboard
    {
        match ui_state.get() {
            UiState::Playing => {
                next_ui.set(UiState::PauseMenu);
                next_sim.set(SimState::Paused);
            }
            _ => {
                next_ui.set(UiState::Playing);
                next_sim.set(SimState::Playing);
            }
        }
    }

    let egui_wants_pointer = egui_wants_input.map_or(false, |e| e.wants_any_input());
    if active_bindings.just_pressed(
        InputAction::CaptureCursor,
        &keys,
        &mouse,
        active_gamepad(gamepads.iter()),
    ) && !egui_wants_pointer
    {
        next_ui.set(UiState::Playing);
        next_sim.set(SimState::Playing);
    }
}

/// Locks or unlocks the cursor based on the current game and UI state.
fn sync_cursor_lock(
    game_state: Res<State<GameState>>,
    ui_state: Res<State<UiState>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    mut cursor_options: Single<&mut CursorOptions>,
) {
    let egui_wants_keyboard = egui_wants_input.map_or(false, |e| e.wants_keyboard_input());
    let should_lock = matches!(
        game_state.get(),
        GameState::SinglePlayer | GameState::Multiplayer
    ) && *ui_state.get() == UiState::Playing
        && !egui_wants_keyboard;
    cursor_options.visible = !should_lock;
    cursor_options.grab_mode = if should_lock {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
}
