use bevy::prelude::*;
use common::game_state::GameState;

#[cfg(feature = "client")]
use crate::resources::{LastServerState, PendingReconciliation};
#[cfg(feature = "client")]
pub use crate::runtime_client::{ClientSessionPlugin, cleanup_world};
#[cfg(feature = "client")]
pub(crate) use crate::runtime_client::{ClientSessionState, handle_file_data, handle_map_hash};
#[cfg(not(feature = "client"))]
pub use crate::runtime_server::ServerSessionPlugin;

pub(crate) fn configure_authority_sets(app: &mut App) {
    app.configure_sets(
        FixedUpdate,
        (
            game_objects::AuthoritySet::Health,
            game_objects::AuthoritySet::Projectile,
            game_objects::AuthoritySet::Level,
        )
            .run_if(has_authority),
    )
    .configure_sets(
        common::slow_update::SlowUpdate,
        game_objects::AuthoritySet::Level.run_if(has_authority),
    )
    .configure_sets(
        common::slow_update::SemiSlowUpdate,
        game_objects::AuthoritySet::Level.run_if(has_authority),
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

#[cfg(feature = "client")]
pub fn snapshot_server_state(
    pending: Res<PendingReconciliation>,
    mut last: ResMut<LastServerState>,
) {
    if let Some(st) = &pending.0 {
        last.0 = Some(st.clone());
    }
}
