// This file declares a AppSettings resource and accompanying plugin for dealing with in-app settings

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Resource)]
pub struct AppSettings {
    volume: i8, // possibly clamp to 0-100
}
impl Default for AppSettings {
    fn default() -> Self {
        AppSettings { volume: 50 }
    }
}

// resource used by the client to handle settings
pub struct AppSettingsPlugin;
impl Plugin for AppSettingsPlugin {
    fn build(&self, app: &mut App) {
        // app.init_resource()
        app.insert_resource(AppSettings::default());
        // app.add_systems(PreStartup, init_window);
        // app.add_systems(Update, toggle_cursor_lock);
    }
}
