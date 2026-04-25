use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use common::{LeaderboardScope, ScoringOption};
use game_objects::pawn::Possessed;
use net::message::{NetworkID, ScoreboardEntry};
use session::GuiState;

pub fn gui_scoreboard(
    mut contexts: EguiContexts,
    gui: Res<GuiState>,
    possessed: Query<&NetworkID, With<Possessed>>,
) {
    let Some(snapshot) = gui.scoreboard.as_ref() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let local_net_id = possessed.single().ok().cloned();
    egui::Window::new("scoreboard")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 10.0))
        .show(ctx, |ui| {
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
