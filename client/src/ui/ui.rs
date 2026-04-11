use crate::settings::Settings;
use crate::{GameState, UiState};
use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use bevy_steamworks::Client;
use common::tick::NetworkStats;
use common::{ActiveKeyBindings, InputAction, LeaderboardScope, ScoringOption};
use game_objects::health::Health;
use game_objects::messages::{GameMessages, MESSAGE_TTL_SECS};
use game_objects::pawn::biped::{BipedPawnComponent, PitchPivot, WeaponSlots};
use game_objects::pawn::{Possessed, VehicleComponent};
use game_objects::weapon::{AimReticle, WeaponConfig, WeaponState, default_crosshair_path};
use net::message::{MsgType, NetworkID, ScoreboardEntry};
use net::quic::{Channel, QuicManager, SendTarget};
use physics::physics_world::{PhysicsWorld, rb_vel};
use session::{GuiState, PendingExit};

pub struct UIPlugin;

#[derive(Resource, Default)]
struct SmoothedFps(Option<f32>);

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_crosshair, spawn_prediction_reticle))
            .add_systems(EguiPrimaryContextPass, set_style.run_if(run_once))
            .add_plugins(EguiPlugin::default())
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .init_resource::<SmoothedFps>()
            .add_systems(Update, update_smoothed_fps)
            .add_systems(EguiPrimaryContextPass, gui_top_left)
            .add_systems(EguiPrimaryContextPass, gui_notifications)
            .add_systems(
                EguiPrimaryContextPass,
                gui_chat.run_if(in_state(GameState::Multiplayer)),
            )
            .add_systems(EguiPrimaryContextPass, gui_scoreboard)
            .add_systems(EguiPrimaryContextPass, gui_health)
            .add_systems(EguiPrimaryContextPass, gui_ammo)
            .add_systems(Update, (update_reticle, update_prediction_reticle));
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
    smoothed_fps: Res<SmoothedFps>,
    net_stats: Res<NetworkStats>,
    settings: Res<Settings>,
    game_state: Res<State<GameState>>,
    mut next_game: ResMut<NextState<GameState>>,
    mut next_ui: ResMut<NextState<UiState>>,
    mut pending_exit: ResMut<PendingExit>,
    mut exit: MessageWriter<AppExit>,
) -> Result {
    if !settings.debug_panel {
        return Ok(());
    }
    egui::Window::new("info")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(10.0, 10.0))
        .show(contexts.ctx_mut()?, |ui| {
            ui.label(format!("rigidbodies: {}", &world.rigid_body_set.len()));
            if let Some(fps) = smoothed_fps.0 {
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

fn update_smoothed_fps(
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    mut smoothed_fps: ResMut<SmoothedFps>,
) {
    let Some(raw_fps) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.value())
        .map(|fps| fps as f32)
        .filter(|fps| fps.is_finite() && *fps > 0.0)
    else {
        return;
    };
    const HALF_LIFE_SECS: f32 = 0.35;
    let alpha = 1.0 - f32::exp2(-time.delta_secs() / HALF_LIFE_SECS);
    smoothed_fps.0 = Some(match smoothed_fps.0 {
        Some(prev) => prev + (raw_fps - prev) * alpha,
        None => raw_fps,
    });
}

fn gui_chat(
    mut contexts: EguiContexts,
    mut state: ResMut<GuiState>,
    mut quic: ResMut<QuicManager>,
    steam: Option<Res<Client>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<ActiveKeyBindings>,
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

            if bindings.just_pressed(InputAction::Chat, &keys, &mouse)
                && !ctx.wants_keyboard_input()
            {
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

fn gui_ammo(
    mut contexts: EguiContexts,
    slots_q: Query<&WeaponSlots, With<Possessed>>,
    weapon_q: Query<(&WeaponState, &WeaponConfig)>,
) {
    let Some(weapon_entity) = slots_q.single().ok().and_then(|slots| slots.active().1) else {
        return;
    };
    let Ok((state, config)) = weapon_q.get(weapon_entity) else {
        return;
    };
    egui::Window::new("ammo")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -10.0))
        .show(contexts.ctx_mut().unwrap(), |ui| {
            ui.heading(format!("{}/{}", state.ammo_in_mag, state.reserve_ammo));
            if state.reload_ticks > 0 && config.reload_ticks > 0 {
                let progress = 1.0 - state.reload_ticks as f32 / config.reload_ticks.max(1) as f32;
                ui.label(format!(
                    "Reloading {:.0}%",
                    progress.clamp(0.0, 1.0) * 100.0
                ));
            }
        });
}

fn gui_scoreboard(
    mut contexts: EguiContexts,
    gui: Res<GuiState>,
    possessed: Query<&NetworkID, With<Possessed>>,
) {
    let Some(snapshot) = gui.scoreboard.as_ref() else {
        return;
    };
    let local_net_id = possessed.single().ok().cloned();
    egui::Window::new("scoreboard")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 10.0))
        .show(contexts.ctx_mut().unwrap(), |ui| {
            ui.label(&snapshot.primary_objective_label);
            match snapshot.scoring {
                ScoringOption::Unscored => ui.label(&snapshot.leaderboard_label),
                ScoringOption::ScoreToWin(target) => {
                    ui.label(format!("{} to {}", snapshot.leaderboard_label, target))
                }
            };
            ui.separator();
            let rows = match snapshot.leaderboard_scope {
                LeaderboardScope::None => return,
                LeaderboardScope::Player => &snapshot.players,
                LeaderboardScope::Team => &snapshot.teams,
            };
            for entry in sorted_rows(rows) {
                let is_local = local_net_id.as_ref().is_some_and(|net_id| {
                    snapshot.leaderboard_scope == LeaderboardScope::Player
                        && entry.net_id == *net_id
                });
                let text = format!("{}  {}", entry.label, entry.value);
                if is_local {
                    ui.colored_label(egui::Color32::from_rgb(255, 220, 120), text);
                } else {
                    ui.label(text);
                }
            }
        });
}

fn sorted_rows(rows: &[ScoreboardEntry]) -> Vec<&ScoreboardEntry> {
    let mut sorted = rows.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));
    sorted
}

fn gui_notifications(
    mut contexts: EguiContexts,
    time: Res<Time>,
    mut messages: ResMut<GameMessages>,
) {
    let now = time.elapsed_secs_f64();
    messages
        .0
        .retain(|entry| now - entry.created_at < MESSAGE_TTL_SECS);
    if messages.0.is_empty() {
        return;
    }
    egui::Window::new("notifications")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::TRANSPARENT)
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::ZERO),
        )
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 70.0))
        .show(contexts.ctx_mut().unwrap(), |ui| {
            ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
            for entry in &messages.0 {
                let age = (now - entry.created_at).max(0.0);
                let alpha = (1.0 - age / MESSAGE_TTL_SECS).clamp(0.0, 1.0);
                let color =
                    egui::Color32::from_rgba_premultiplied(255, 255, 255, (alpha * 255.0) as u8);
                ui.colored_label(color, &entry.text);
            }
        });
}

/// Updates the crosshair image from the active controllable object.
fn update_reticle(
    biped: Query<&WeaponSlots, With<Possessed>>,
    vehicle: Query<&AimReticle, (With<Possessed>, With<VehicleComponent>)>,
    reticles: Query<&AimReticle>,
    mut crosshair: Query<&mut ImageNode, With<Crosshair>>,
    asset_server: Res<AssetServer>,
    mut current: Local<Option<&'static str>>,
) {
    let path = vehicle
        .single()
        .ok()
        .map(|reticle| reticle.0)
        .or_else(|| {
            let slots = biped.single().ok()?;
            let weapon_entity = slots.active().1?;
            reticles.get(weapon_entity).ok().map(|reticle| reticle.0)
        })
        .unwrap_or(default_crosshair_path());
    if *current == Some(path) {
        return;
    }
    if let Ok(mut img) = crosshair.single_mut() {
        img.image = asset_server.load(path);
        *current = Some(path);
    }
}

#[derive(Component)]
pub struct PredictionReticle;

fn spawn_prediction_reticle(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        PredictionReticle,
        ImageNode {
            image: asset_server.load("textures/crosshairs/crosshair030.png"),
            color: Color::srgba(1.0, 1.0, 1.0, 0.2),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(18.0),
            height: Val::Px(18.0),
            ..default()
        },
        ZIndex(10),
        Visibility::Hidden,
    ));
}

fn update_prediction_reticle(
    pawn: Query<(Entity, &WeaponSlots, &BipedPawnComponent), With<Possessed>>,
    pitch_pivots: Query<&GlobalTransform, With<PitchPivot>>,
    weapons: Query<&AimReticle>,
    targets: Query<(Entity, &GlobalTransform, &Health), Without<Possessed>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    world: Res<PhysicsWorld>,
    mut indicator: Query<(&mut Node, &mut Visibility), With<PredictionReticle>>,
) {
    let Ok((mut node, mut vis)) = indicator.single_mut() else {
        return;
    };
    let show = (|| -> Option<Vec2> {
        let (pawn_entity, slots, biped) = pawn.single().ok()?;
        let weapon_entity = slots.active().1?;
        let projectile_speed = weapons.get(weapon_entity).ok()?.1?;
        let pitch_pivot = biped.pitch_pivot?;
        let origin = pitch_pivots.get(pitch_pivot).ok()?.translation();
        let (camera, camera_gt) = camera.single().ok()?;
        let viewport_size = camera.logical_viewport_size()?;
        let viewport_center = viewport_size * 0.5;
        let shooter_velocity = world
            .entity_to_handle
            .get(&pawn_entity)
            .and_then(|&h| world.rigid_body_set.get(h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);

        let mut best = None;
        let mut best_score = f32::INFINITY;
        for (target_entity, target_gt, health) in targets.iter() {
            if health.current <= 0.0 {
                continue;
            }
            let target_pos = target_gt.translation();
            let Ok(screen_pos) = camera.world_to_viewport(camera_gt, target_pos) else {
                continue;
            };
            if world
                .cast_ray(
                    origin,
                    (target_pos - origin).normalize_or_zero(),
                    origin.distance(target_pos),
                    &[pawn_entity],
                )
                .is_some_and(|(hit, _)| hit != target_entity)
            {
                continue;
            }
            let score = screen_pos.distance_squared(viewport_center);
            if score >= best_score {
                continue;
            }
            let target_velocity = world
                .entity_to_handle
                .get(&target_entity)
                .and_then(|&h| world.rigid_body_set.get(h))
                .map(rb_vel)
                .unwrap_or(Vec3::ZERO);
            let relative_position = target_pos - origin;
            let relative_velocity = target_velocity - shooter_velocity;
            let Some(time) =
                solve_intercept_time(relative_position, relative_velocity, projectile_speed)
            else {
                continue;
            };
            let relative_intercept = relative_position + relative_velocity * time;
            let aim_point = origin + relative_intercept;
            let Ok(intercept_screen) = camera.world_to_viewport(camera_gt, aim_point) else {
                continue;
            };
            best_score = score;
            best = Some(intercept_screen);
        }
        best
    })();
    match show {
        Some(pos) => {
            node.left = Val::Px(pos.x - 12.0);
            node.top = Val::Px(pos.y - 12.0);
            *vis = Visibility::Inherited;
        }
        None => *vis = Visibility::Hidden,
    }
}

fn solve_intercept_time(
    relative_position: Vec3,
    relative_velocity: Vec3,
    speed: f32,
) -> Option<f32> {
    let a = relative_velocity.length_squared() - speed * speed;
    let b = 2.0 * relative_position.dot(relative_velocity);
    let c = relative_position.length_squared();
    if c <= f32::EPSILON {
        return Some(0.0);
    }
    if a.abs() <= f32::EPSILON {
        if b.abs() <= f32::EPSILON {
            return None;
        }
        let t = -c / b;
        return (t > 0.0).then_some(t);
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let t0 = (-b - root) / (2.0 * a);
    let t1 = (-b + root) / (2.0 * a);
    [t0, t1]
        .into_iter()
        .filter(|t| *t > 0.0 && t.is_finite())
        .min_by(f32::total_cmp)
}

#[derive(Component)] // query for this component when removing it
pub struct Crosshair;
pub fn spawn_crosshair(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Crosshair,
        ImageNode::new(asset_server.load(default_crosshair_path())),
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
