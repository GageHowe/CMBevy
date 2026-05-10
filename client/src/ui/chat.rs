use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use common::{ActiveBindings, InputAction, active_gamepad};
use net::{
    message::MsgType,
    quic::{Channel, QuicManager},
};
use session::GuiState;

use crate::steam::SteamClient;

pub fn gui_chat(
    mut contexts: EguiContexts,
    mut state: ResMut<GuiState>,
    mut quic: ResMut<QuicManager>,
    steam: Option<Res<SteamClient>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<ActiveBindings>,
    gamepads: Query<&Gamepad>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    egui::Window::new("chat")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(10.0, -10.0))
        .min_width(400.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .auto_shrink([false, true])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &state.log {
                        ui.label(line);
                    }
                });

            ui.separator();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.command_input)
                    .hint_text("press T to chat")
                    .desired_width(f32::INFINITY),
            );

            if bindings.just_pressed(
                InputAction::Chat,
                &keys,
                &mouse,
                active_gamepad(gamepads.iter()),
            ) && !ctx.wants_keyboard_input()
            {
                resp.request_focus();
            }

            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let txt = state.command_input.trim().to_string();
                if !txt.is_empty() {
                    let name = steam
                        .as_ref()
                        .map(|s| s.friends().name())
                        .unwrap_or_else(|| "Player".to_string());
                    quic.send_to_server(Channel::Ordered, &MsgType::ChatMessage(name, txt));
                }
                state.command_input.clear();
            }

            if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                state.command_input.clear();
                resp.surrender_focus();
            }
        });
}
