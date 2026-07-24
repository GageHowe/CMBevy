/*
atmosphere-adjacent zones:
* area reverb drives client audio based on listener proximity
*/

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub struct AtmospherePlugin;
impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<AreaReverbComponent>();
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct AreaReverbComponent {
    /// full-effect radius
    pub min_distance: f32,
    /// fade-out radius
    pub max_distance: f32,
}
