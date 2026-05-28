use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use gameplay::{
    health::Health,
    messages::{GameMessages, MESSAGE_TTL_SECS},
    pawn::{InteractionHint, Possessed, WeaponSlots, biped::BipedPawnComponent},
    weapon::{WeaponConfig, WeaponState},
};

pub fn gui_health(mut contexts: EguiContexts, health_q: Query<&Health, With<Possessed>>) {
    let Ok(health) = health_q.single() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let fraction = (health.current as f32 / health.max as f32).clamp(0.0, 1.0);
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
        .show(ctx, |ui| {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .fill(bar_color)
                    .desired_width(100.0)
                    .desired_height(10.0)
                    .corner_radius(0.0),
            );
        });
}

pub fn gui_ability_status(
    mut contexts: EguiContexts,
    biped_q: Query<&BipedPawnComponent, With<Possessed>>,
) {
    let Ok(biped) = biped_q.single() else {
        return;
    };
    let Some(ability) = &biped.ability else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    egui::Window::new("ability_status")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 26.0))
        .show(ctx, |ui| {
            ui.add(
                egui::ProgressBar::new(ability.status_fraction())
                    .fill(egui::Color32::WHITE)
                    .desired_width(100.0)
                    .desired_height(10.0)
                    .corner_radius(0.0)
                    .show_percentage(),
            );
        });
}

pub fn gui_ammo(
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
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    egui::Window::new("ammo")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -10.0))
        .show(ctx, |ui| {
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

pub fn gui_notifications(
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
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
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
        .show(ctx, |ui| {
            for entry in &messages.0 {
                let age = (now - entry.created_at).max(0.0);
                let alpha = (1.0 - age / MESSAGE_TTL_SECS).clamp(0.0, 1.0);
                let color =
                    egui::Color32::from_rgba_premultiplied(255, 255, 255, (alpha * 255.0) as u8);
                ui.colored_label(color, &entry.text);
            }
        });
}

pub fn gui_interaction_hint(mut contexts: EguiContexts, hint: Res<InteractionHint>) {
    let Some(text) = hint.0.as_ref() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    egui::Window::new("interaction_hint")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_premultiplied(10, 0, 10, 100))
                .corner_radius(egui::CornerRadius::same(4)),
        )
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -80.0))
        .show(ctx, |ui| {
            ui.label(text);
        });
}
