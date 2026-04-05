use crate::session::PendingExit;
use crate::{GameState, UiState};
use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use bevy_steamworks::Client;
use common::tick::NetworkStats;
use game_objects::health::Health;
use game_objects::pawn::Possessed;
use game_objects::pawn::biped::WeaponSlots;
use game_objects::weapon::{WeaponCrosshair, default_crosshair_path};
use net::message::MsgType;
use net::quic::{Channel, QuicManager, SendTarget};
use physics::physics_world::PhysicsWorld;

#[derive(Resource, Debug, Default)]
/// Data that needs to persist inside the GUI (text etc)
pub struct GuiState {
    // pub text_input: String,
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
            .add_systems(EguiPrimaryContextPass, set_style.run_if(run_once))
            .insert_resource(GuiState::default())
            .add_plugins(EguiPlugin::default())
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(EguiPrimaryContextPass, gui_top_left)
            .add_systems(
                EguiPrimaryContextPass,
                gui_chat.run_if(in_state(GameState::Multiplayer)),
            )
            .add_systems(EguiPrimaryContextPass, gui_health)
            .add_systems(Update, update_reticle);
    }
}

fn set_style(mut contexts: EguiContexts) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "JetBrainsMono-Light".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../../assets/fonts/JetBrainsMono-Light.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "JetBrainsMono-Light".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .insert(0, "JetBrainsMono-Light".to_owned());
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.visuals.window_shadow = egui::epaint::Shadow::NONE;
    style.visuals.window_fill = egui::Color32::from_rgba_premultiplied(10, 0, 10, 100);
    style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.override_text_color = Some(egui::Color32::WHITE);
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.widgets.noninteractive.bg_fill =
        egui::Color32::from_rgba_premultiplied(20, 0, 20, 160);
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    // style.visuals.window_stroke = egui::Stroke { width: 1.0, color: egui::Color32::BLACK };
    style.visuals.window_stroke = egui::Stroke {
        width: 0.0,
        color: egui::Color32::TRANSPARENT,
    };
    ctx.set_style(style);
}

fn gui_top_left(
    mut contexts: EguiContexts,
    world: ResMut<PhysicsWorld>,
    diagnostics: Res<DiagnosticsStore>,
    net_stats: Res<NetworkStats>,
    game_state: Res<State<GameState>>,
    mut next_game: ResMut<NextState<GameState>>,
    mut next_ui: ResMut<NextState<UiState>>,
    mut pending_exit: ResMut<PendingExit>,
    mut exit: MessageWriter<AppExit>,
) -> Result {
    egui::Window::new("info")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(10.0, 10.0))
        .show(contexts.ctx_mut()?, |ui| {
            ui.label(format!("rigidbodies: {}", &world.rigid_body_set.len()));
            if let Some(fps) = diagnostics
                .get(&FrameTimeDiagnosticsPlugin::FPS)
                .and_then(|d| d.smoothed())
            {
                ui.label(format!("FPS: {fps:.0}"));
            } else {
                ui.label("FPS: N/A");
            }

            if net_stats.rtt_secs > 0.0 {
                let half_rtt_ticks =
                    (net_stats.rtt_secs * common::config::FIXED_TICK_RATE as f32 * 0.5).ceil();
                ui.label(format!(
                    "RTT: {:.0} ms  predict: +{half_rtt_ticks:.0} ticks",
                    net_stats.rtt_secs * 1000.0
                ));
            } else {
                ui.label("RTT: --");
            }
            if ui.button("Quit").clicked() {
                if *game_state.get() == GameState::MainMenu {
                    exit.write(AppExit::Success);
                } else {
                    // Route shutdown through MainMenu so OnExit cleanup runs before process exit.
                    pending_exit.0 = true;
                    next_game.set(GameState::MainMenu);
                    next_ui.set(UiState::Playing);
                }
            }
        });
    Ok(())
}

fn gui_chat(
    mut contexts: EguiContexts,
    mut state: ResMut<GuiState>,
    mut quic: ResMut<QuicManager>,
    steam: Option<Res<Client>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let ctx = contexts.ctx_mut().unwrap();
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

            if keys.just_pressed(KeyCode::KeyT) && !ctx.wants_keyboard_input() {
                resp.request_focus();
            }

            // TextEdit surrenders focus on Enter internally, so check lost_focus.
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let txt = state.command_input.trim().to_string();
                if !txt.is_empty() {
                    let name = steam
                        .as_ref()
                        .map(|s| s.friends().name())
                        .unwrap_or_else(|| "Player".to_string());
                    quic.send(
                        SendTarget::All,
                        Channel::Ordered,
                        &MsgType::ChatMessage(name, txt),
                    );
                }
                state.command_input.clear();
            }

            // Escape is not handled by TextEdit, so surrender focus manually.
            if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                state.command_input.clear();
                resp.surrender_focus();
            }
        });
}

fn gui_health(mut contexts: EguiContexts, health_q: Query<&Health, With<Possessed>>) {
    let Ok(health) = health_q.single() else {
        return;
    };
    let fraction = (health.current / health.max).clamp(0.0, 1.0);
    let bar_color = if fraction > 0.5 {
        egui::Color32::from_rgb(80, 200, 80)
    } else if fraction > 0.25 {
        egui::Color32::from_rgb(220, 180, 0)
    } else {
        egui::Color32::from_rgb(220, 60, 60)
    };
    egui::Window::new("health")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 10.0))
        .show(contexts.ctx_mut().unwrap(), |ui| {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .fill(bar_color)
                    .desired_width(110.0),
            );
            ui.label(format!("{:.0} / {:.0}", health.current, health.max));
        });
}

/// Updates the crosshair image when the active weapon slot changes.
fn update_reticle(
    possessed: Query<&WeaponSlots, (With<Possessed>, Changed<WeaponSlots>)>,
    crosshairs: Query<&WeaponCrosshair>,
    mut crosshair: Query<&mut ImageNode, With<Crosshair>>,
    asset_server: Res<AssetServer>,
) {
    let Ok(slots) = possessed.single() else {
        return;
    };
    let path = if let Some(e) = slots.active().1 {
        crosshairs
            .get(e)
            .map(|crosshair| crosshair.0)
            .unwrap_or("textures/crosshairs/crosshair013.png")
    } else {
        default_crosshair_path()
    };
    if let Ok(mut img) = crosshair.single_mut() {
        img.image = asset_server.load(path);
    }
}

#[derive(Component)] // query for this component when removing it
pub struct Crosshair;
pub fn spawn_crosshair(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Crosshair,
        ImageNode::new(asset_server.load("textures/crosshairs/crosshair022.png")),
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
    ));
}
