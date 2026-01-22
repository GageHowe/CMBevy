use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use cmbevy::core::level::level::*;
use cmbevy::core::physics::physics_world::*;
use cmbevy::core::player::player::*;
fn main() {
    App::new()
        // dirty workaround from https://taintedcoders.com/bevy/how-to/headless-mode
        .add_plugins(
            DefaultPlugins
                // .set(ScheduleRunnerPlugin::run_once())
                .set(RenderPlugin {
                    // synchronous_pipeline_compilation: true,
                    render_creation: RenderCreation::Automatic(WgpuSettings {
                        backends: None,
                        ..default()
                    }),
                    ..default()
                }),
        )
        // .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .add_plugins(PhysicsPlugin)
        .add_plugins(PlayerPlugin)
        .add_plugins(LevelPlugin)
        .run();
}
