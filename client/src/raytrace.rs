use bevy::prelude::*;
use bevy_luna::prelude::RaytracePlugins;

pub struct RaytraceTogglePlugin;

impl Plugin for RaytraceTogglePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RaytracePlugins);
    }
}
