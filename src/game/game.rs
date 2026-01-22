use bevy::prelude::*;

use crate::game::{physics::physics::PhysicsPlugin, window::WindowSettingsPlugin};

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(WindowSettingsPlugin);
        app.add_plugins(PhysicsPlugin);
    }
}
