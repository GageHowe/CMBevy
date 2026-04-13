mod helpers;
mod messages;
mod resources;
mod runtime;

#[cfg(not(feature = "client"))]
mod actions;
#[cfg(not(feature = "client"))]
mod connections;
#[cfg(feature = "client")]
mod hosted;
#[cfg(not(feature = "client"))]
mod replication;

#[cfg(feature = "client")]
pub use hosted::{
    available_gametypes, available_maps, cleanup_before_app_exit, exit_after_returning_to_menu,
    fetch_lan_lobbies, fetch_remote_lobbies, gametype_path, shutdown_session, start_hosted_server,
};
#[cfg(feature = "client")]
pub use messages::draw_server_state;
#[cfg(not(feature = "client"))]
pub use messages::on_message;
#[cfg(feature = "client")]
pub use resources::{
    GuiState, HostedServer, LastAckedInputSeq, LastServerState, PendingExit, PendingReconciliation,
    PendingWorldReady, ServerAddr, SinglePlayerConfig,
};
#[cfg(not(feature = "client"))]
pub use runtime::ServerSessionPlugin;
#[cfg(feature = "client")]
pub use runtime::{ClientSessionPlugin, cleanup_world, has_authority, snapshot_server_state};
#[cfg(not(feature = "client"))]
pub use runtime::has_authority;
