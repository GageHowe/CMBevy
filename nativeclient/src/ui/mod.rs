mod chat;
mod debug;
mod hud;
mod reticle;
mod scoreboard;
pub mod window;

use bevy::{diagnostic::FrameTimeDiagnosticsPlugin, prelude::*};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};

use crate::{GameState, settings::Settings};

pub struct UIPlugin;

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            // plugins + resources
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .init_resource::<debug::SmoothedFps>()
            // Startup
            .add_systems(
                Startup,
                (reticle::spawn_crosshair, reticle::spawn_prediction_reticle),
            )
            // Update
            .add_systems(Update, debug::update_smoothed_fps)
            .add_systems(
                Update,
                (reticle::update_reticle, reticle::update_prediction_reticle),
            )
            .add_systems(
                Update,
                reticle::apply_reticle_scale.run_if(resource_changed::<Settings>),
            )
            // EguiPrimaryContextPass
            .add_systems(EguiPrimaryContextPass, set_style.run_if(run_once))
            .add_systems(EguiPrimaryContextPass, debug::debug_panel)
            .add_systems(EguiPrimaryContextPass, hud::gui_notifications)
            .add_systems(EguiPrimaryContextPass, hud::gui_interaction_hint)
            .add_systems(
                EguiPrimaryContextPass,
                chat::gui_chat.run_if(in_state(GameState::Multiplayer)),
            )
            .add_systems(EguiPrimaryContextPass, scoreboard::gui_scoreboard)
            .add_systems(EguiPrimaryContextPass, hud::gui_health)
            .add_systems(EguiPrimaryContextPass, hud::gui_ability_status)
            .add_systems(EguiPrimaryContextPass, hud::gui_ammo);
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
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "JetBrainsMono-Light".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.insert(0, "JetBrainsMono-Light".to_owned());
    }
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(5.0, 2.0);
    style.spacing.window_margin = egui::Margin::same(8);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.indent = 12.0;
    style.visuals.window_shadow = egui::epaint::Shadow::NONE;
    style.visuals.window_fill = egui::Color32::from_rgba_premultiplied(10, 0, 10, 100);
    style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.override_text_color = Some(egui::Color32::WHITE);
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.widgets.noninteractive.bg_fill =
        egui::Color32::from_rgba_premultiplied(20, 0, 20, 160);
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    style.visuals.window_stroke = egui::Stroke {
        width: 0.0,
        color: egui::Color32::TRANSPARENT,
    };
    ctx.set_style(style);
}
