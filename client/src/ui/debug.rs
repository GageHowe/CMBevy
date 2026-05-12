use bevy::{
    app::AppExit,
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    post_process::auto_exposure::AutoExposure,
    render::view::Hdr,
};
use bevy_egui::{EguiContexts, egui};
use common::tick::NetworkStats;
use physics::physics_world::PhysicsWorld;
use session::PendingExit;

use crate::{
    GameState, UiState, auto_exposure_debug::AutoExposureCorrection, settings::Settings,
};

#[derive(Resource, Default)]
pub struct SmoothedFps(pub Option<f32>);

/// top left debug panel for showing debug info
pub fn debug_panel(
    mut contexts: EguiContexts,
    world: ResMut<PhysicsWorld>,
    smoothed_fps: Res<SmoothedFps>,
    net_stats: Res<NetworkStats>,
    settings: Res<Settings>,
    auto_exposure_correction: Res<AutoExposureCorrection>,
    camera_q: Query<(Has<AutoExposure>, Has<Hdr>), With<Camera3d>>,
    _game_state: Res<State<GameState>>,
    _next_game: ResMut<NextState<GameState>>,
    _next_ui: ResMut<NextState<UiState>>,
    _pending_exit: ResMut<PendingExit>,
    _exit: MessageWriter<AppExit>,
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
            ui.label(format!(
                "packet: {}",
                net::format_packet_size(net_stats.last_packet_bytes)
            ));
            if let Ok((auto_exposure, hdr)) = camera_q.single() {
                let mode = if auto_exposure { "auto" } else { "manual" };
                let hdr = if hdr { "hdr" } else { "ldr" };
                let correction = auto_exposure_correction
                    .0
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| "N/A".to_string());
                ui.label(format!("exposure correction: {correction} ({mode}, {hdr})"));
            } else {
                ui.label("exposure correction: N/A");
            }
        });
    Ok(())
}

pub fn update_smoothed_fps(
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
    const HALF_LIFE_SECS: f32 = 0.35; // subject to tuning
    let alpha = 1.0 - f32::exp2(-time.delta_secs() / HALF_LIFE_SECS);
    smoothed_fps.0 = Some(match smoothed_fps.0 {
        Some(prev) => prev + (raw_fps - prev) * alpha,
        None => raw_fps,
    });
}
