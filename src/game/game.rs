use bevy::prelude::*;

use crate::game::level::level::LevelPlugin;
use crate::game::player::player::PlayerPlugin;
use crate::game::{physics::physics::PhysicsPlugin, window::WindowSettingsPlugin};

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(WindowSettingsPlugin);
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        // app.add_plugins(PhysicsPlugin);
        app.add_plugins(PlayerPlugin);
        app.add_plugins(LevelPlugin);
    }
}
