use bevy::prelude::*;
// use bevy::ui::
// use bevy::window::*;
// use crate::client::ClientNetManager;
use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use crate::debug_println;
// use common::net::net::{MsgType, str_to_message};
// use common::net::runtime::TokioRuntimePlugin;
// use crate::physics::physics_world::*;
// use common::
use std::hint::unlikely;
use crate::net::quic::{QuicManager, SendTarget, Channel};
use crate::net::message::MsgType;
use crate::physics::physics_world::PhysicsWorld;

#[derive(Resource, Debug, Default)]
/// Data that needs to persist inside the GUI (text etc)
pub struct GuiState {
    pub text_input: String,
    pub command_input: String,
    pub log: Vec<String>,
}
impl GuiState {
    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }
}

pub struct UIPlugin;

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crosshair)
            .insert_resource(GuiState::default())
            .add_plugins(EguiPlugin::default())
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(EguiPrimaryContextPass, gui_top_left)
            .add_systems(EguiPrimaryContextPass, gui_bottom_left)
            .add_systems(EguiPrimaryContextPass, gui_log)
            ;
    }
}

fn gui_top_left(
    mut contexts: EguiContexts,
    world: ResMut<PhysicsWorld>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
    mut style_set: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    if unlikely(!*style_set) {
        let mut style = (*ctx.style()).clone();
        // style.visuals.window_shadow = egui::epaint::Shadow::NONE;
        style.visuals.window_fill = egui::Color32::from_rgba_premultiplied(10, 0, 10, 200);
        style.visuals.override_text_color = Some(egui::Color32::WHITE);
        style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
        style.visuals.window_stroke = egui::Stroke { width: 1.0, color: egui::Color32::BLACK };
        ctx.set_style(style);
        *style_set = true;
        // debug_println!("set style")
    }

    egui::Window::new("info")
        .title_bar(false)
        .resizable(false)
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(10.0, 10.0))
        .show(contexts.ctx_mut()?, |ui| {
            ui.label("bevy_egui test");
            ui.label(format!("rigidbodies: {}", &world.rigid_body_set.len()));
            if let Some(fps) = diagnostics
                .get(&FrameTimeDiagnosticsPlugin::FPS)
                .and_then(|d| d.smoothed())
            {
                ui.label(format!("FPS: {fps:.0}"));
            } else {
                ui.label("FPS: N/A");
            }

            if ui.button("Quit").clicked() {
                exit.write(AppExit::Success);
            }
        });
    Ok(())
}

fn gui_bottom_left(
    mut contexts: EguiContexts,
    mut state: ResMut<GuiState>,
    mut quic: ResMut<QuicManager>,
) {
    let ctx = contexts.ctx_mut().unwrap();

    egui::Window::new("messagebar")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(10.0, -10.0))
        .show(ctx, |ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.command_input)
                    // .hint_text("")
                    .desired_width(200.0),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let txt = state.command_input.trim().to_string();
                if !txt.is_empty() {
                    quic.send(
                        SendTarget::All,
                        Channel::Ordered,
                        &MsgType::ChatMessage("".to_string(), txt),
                    );
                }
                state.command_input.clear();
                resp.request_focus();
            }
        });
}

fn gui_log(
    mut contexts: EguiContexts,
    state: Res<GuiState>,
) {
    egui::Window::new("log")
        .title_bar(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(10.0, -10.0))
        .fixed_size([400.0, 150.0])
        .show(contexts.ctx_mut().unwrap(), |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &state.log {
                        ui.label(line);
                    }
                });
        });
}

#[derive(Component)] // query for this component when removing it
pub struct Crosshair;
pub fn spawn_crosshair(mut commands: Commands, asset_server: Res<AssetServer>) {
    let crosshair_size = 2.0;
    commands
        .spawn((
            Crosshair,
            ImageNode::new(asset_server.load("crosshairs/crosshair010.png")),
            Node {
                width: Val::Px(32.0),
                height: Val::Px(32.0),
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                margin: UiRect {
                    left: Val::Px(-16.0),
                    top: Val::Px(-16.0),
                    ..default()
                },
                ..default()
            },
            // BackgroundColor(Color::NONE),
        ))
        .with_children(|parent| {
            parent.spawn((
                Node {
                    width: Val::Px(crosshair_size),
                    height: Val::Px(crosshair_size),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
            ));
        });
}
