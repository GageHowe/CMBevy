use bevy::prelude::*;
use common::game_state::*;

#[cfg(feature = "client")]
pub use crate::session::runtime_client::{ClientSessionPlugin, cleanup_world};
#[cfg(not(feature = "client"))]
pub use crate::session::runtime_server::ServerSessionPlugin;

pub(crate) fn configure_gameplay_sets(app: &mut App) {
    app.configure_sets(
        FixedPreUpdate,
        crate::pawn::GatherInputSet.in_set(SimulationSystems),
    )
    .configure_sets(
        FixedPreUpdate,
        crate::pawn::MovePawnsSet.in_set(SimulationSystems),
    )
    .configure_sets(
        FixedUpdate,
        crate::AuthoritySystems
            .in_set(SimulationSystems)
            .run_if(has_authority),
    )
    .configure_sets(
        common::slow_update::SlowUpdate,
        crate::AuthoritySystems
            .in_set(SimulationSystems)
            .run_if(has_authority),
    )
    .configure_sets(
        common::slow_update::SemiSlowUpdate,
        crate::AuthoritySystems
            .in_set(SimulationSystems)
            .run_if(has_authority),
    );
}

pub fn has_authority(state: Option<Res<State<GameState>>>) -> bool {
    #[cfg(feature = "client")]
    {
        state.is_some_and(|s| *s.get() == GameState::SinglePlayer)
    }
    #[cfg(not(feature = "client"))]
    {
        let _ = state;
        true
    }
}
