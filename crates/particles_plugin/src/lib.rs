#[cfg(feature = "particles")]
pub mod effects;
#[cfg(feature = "particles")]
pub mod helpers;

#[cfg(feature = "particles")]
pub mod prelude {
    pub use hanabi::prelude::*;

    pub use crate::{GPUParticlesPlugin, effects::*};
}

#[cfg(feature = "particles")]
use bevy::prelude::*;

#[cfg(feature = "particles")]
pub struct GPUParticlesPlugin;
#[cfg(feature = "particles")]
impl Plugin for GPUParticlesPlugin {
    fn build(&self, app: &mut App) {
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
