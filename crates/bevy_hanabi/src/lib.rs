use bevy::prelude::*;

pub mod effects;
pub mod helpers;

pub mod prelude {
    pub use hanabi::prelude::*;

    pub use crate::{HanabiEffectsPlugin, effects::*};
}

pub struct HanabiEffectsPlugin;
impl Plugin for HanabiEffectsPlugin {
    fn build(&self, app: &mut App) {
        // Keep the integration point small so effects can live here without leaking Hanabi setup
        // into the client binary or gameplay crates.
        app.add_plugins(hanabi::prelude::HanabiPlugin)
            .init_resource::<effects::rpg::RpgExplosionEffect>()
            .add_systems(Update, helpers::tick_one_shot_effects);
    }
}
