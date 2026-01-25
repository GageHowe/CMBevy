use bevy::prelude::*;
// use bevy::ui::
// use bevy::window::*;
use crate::net::ClientNetManager;
use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use cmbevy::core::net::backend::{Message, str_to_message};
use cmbevy::core::physics::physics_world::*;

#[derive(Resource, Debug, Default)]
/// Data that needs to persist inside the GUI (text etc)
pub struct GuiState {
    pub text_input: String,
    pub command_input: String,
}

pub struct UIPlugin;

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crosshair)
            .insert_resource(GuiState::default())
            .add_plugins(EguiPlugin::default())
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(EguiPrimaryContextPass, gui_top_left)
            .add_systems(EguiPrimaryContextPass, gui_bottom_left);
    }
}

fn gui_top_left(
    mut contexts: EguiContexts,
    world: ResMut<PhysicsWorld>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) -> Result {
    let ctx = contexts.ctx_mut().unwrap();

    // stupid api
    let mut style = (*ctx.style()).clone();
    style.visuals.window_shadow = egui::epaint::Shadow::NONE;
    style.visuals.window_fill = egui::Color32::from_rgba_premultiplied(20, 0, 20, 200);
    style.visuals.override_text_color = Some(egui::Color32::WHITE);
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.window_stroke = egui::Stroke {
        width: 1.0,
        color: egui::Color32::BLACK,
    };
    ctx.set_style(style);

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
    mut net_man: ResMut<ClientNetManager>,
) {
    let ctx = contexts.ctx_mut().unwrap();

    // Anchor bottom-left, a bit from the edge
    egui::Window::new("messagebar")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(10.0, -10.0))
        .show(ctx, |ui| {
            // message input
            let resp_a = ui.add(
                egui::TextEdit::singleline(&mut state.text_input)
                    .hint_text("...")
                    .desired_width(200.0),
            );
            if resp_a.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                println!("text message was sent: {}", state.command_input);

                state.text_input.clear();
                resp_a.request_focus();
            }
            // cmd input
            let resp_b = ui.add(
                egui::TextEdit::singleline(&mut state.command_input)
                    .hint_text("_>")
                    .desired_width(200.0),
            );
            if resp_b.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let txt = &state.command_input;
                println!("command was run: {}", txt);
                let cmd: Message = str_to_message(txt.as_str());
                net_man.enqueue(cmd);

                state.command_input.clear();
                resp_b.request_focus();
            }
        });
}

#[derive(Component)] // query for this component when removing it
pub struct Crosshair;
pub fn spawn_crosshair(mut commands: Commands) {
    let crosshair_size = 2.0;
    commands
        .spawn((
            Crosshair,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
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
