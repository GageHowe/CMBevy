mod chat;
mod debug;
mod hud;
mod reticle;
pub mod window;

use bevy::{
    diagnostic::FrameTimeDiagnosticsPlugin,
    input_focus::{InputFocus, tab_navigation::TabNavigationPlugin},
    pbr::{StandardMaterial, diagnostic::MaterialAllocatorDiagnosticPlugin},
    prelude::*,
    render::diagnostic::MeshAllocatorDiagnosticPlugin,
};
use bevy_dev_tools::diagnostics_overlay::DiagnosticsOverlayPlugin;

use crate::{GameState, UiState, settings::Settings};

pub const UI_FONT: &str = "fonts/JetBrainsMono-Light.ttf";

pub struct UIPlugin;

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<common::UiWantsInput>()
            .add_plugins((
                TabNavigationPlugin,
                FrameTimeDiagnosticsPlugin::default(),
                DiagnosticsOverlayPlugin,
                MeshAllocatorDiagnosticPlugin,
                MaterialAllocatorDiagnosticPlugin::<StandardMaterial>::default(),
            ))
            .add_systems(
                Startup,
                (
                    reticle::spawn_crosshair,
                    reticle::spawn_prediction_reticle,
                    hud::spawn_hud,
                    chat::spawn_chat,
                ),
            )
            .add_systems(
                Update,
                (
                    debug::sync_diagnostics_overlay,
                    sync_ui_wants_input,
                    hud::sync_health,
                    hud::sync_ability,
                    hud::sync_ammo,
                    hud::sync_notifications,
                    hud::sync_interaction_hint,
                    chat::sync_chat,
                    chat::submit_chat.run_if(in_state(GameState::Multiplayer)),
                ),
            )
            .add_systems(
                Update,
                (reticle::update_reticle, reticle::update_prediction_reticle),
            )
            .add_systems(
                Update,
                reticle::apply_reticle_scale.run_if(resource_changed::<Settings>),
            );
    }
}

fn sync_ui_wants_input(
    input_focus: Res<InputFocus>,
    game_state: Res<State<GameState>>,
    ui_state: Res<State<UiState>>,
    mut ui_wants: ResMut<common::UiWantsInput>,
) {
    let menu_open =
        *game_state.get() == GameState::NotPlaying || *ui_state.get() != UiState::Playing;
    let focused = input_focus.get().is_some();
    ui_wants.keyboard = menu_open || focused;
    ui_wants.pointer = menu_open || focused;
}
