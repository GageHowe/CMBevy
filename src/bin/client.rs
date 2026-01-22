use bevy::prelude::*;
use cmbevy::core::level::level::*;
use cmbevy::core::physics::physics_world::*;
use cmbevy::core::player::player::*;
use cmbevy::core::ui::ui::UIPlugin;
use cmbevy::core::window::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .add_plugins(WindowSettingsPlugin)
        .add_plugins(PhysicsPlugin)
        .add_plugins(PlayerPlugin)
        .add_plugins(LevelPlugin)
        .add_plugins(UIPlugin)
        .run();
}
