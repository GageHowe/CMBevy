use bevy::prelude::*;

pub mod effects;
pub mod helpers;

pub mod prelude {
    pub use hanabi::prelude::*;

    pub use crate::{GPUParticlesPlugin, effects::*};
}

pub struct GPUParticlesPlugin;
impl Plugin for GPUParticlesPlugin {
    fn build(&self, app: &mut App) {
        // Keep the integration point small so effects can live here without leaking Hanabi setup
        // into the client binary or gameplay crates.
        app.add_plugins(hanabi::prelude::HanabiPlugin)
            .init_resource::<effects::ability::AbilityEffects>()
            .init_resource::<effects::explosion::CoilLauncherExplosionEffect>()
            .init_resource::<effects::explosion::LobberExplosionEffect>()
            .init_resource::<effects::explosion::SpaceshipDeathExplosionEffect>()
            .init_resource::<effects::explosion::ThumperExplosionEffect>()
            .init_resource::<effects::impact::DustImpactEffect>()
            .init_resource::<effects::impact::SparksImpactEffect>()
            .add_systems(Update, helpers::tick_one_shot_effects);
    }
}
