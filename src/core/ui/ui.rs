use bevy::prelude::*;
// use bevy::ui::
// use bevy::window::*;
use crate::core::physics::physics_world::*;
use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};

pub struct UIPlugin;

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crosshair)
            .add_plugins(EguiPlugin::default())
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(EguiPrimaryContextPass, gui_top_left);

        // app.add_systems(Startup, spawn_game_ui);
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
    style.visuals.window_fill = egui::Color32::from_rgb(20, 0, 20);
    // style.visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK); // border
    style.visuals.override_text_color = Some(egui::Color32::WHITE);
    ctx.set_style(style);

    egui::Window::new("")
        .title_bar(false)
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
