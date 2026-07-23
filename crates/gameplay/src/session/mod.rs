mod helpers;
mod messages;
#[cfg(feature = "client")]
mod messages_client;
#[cfg(not(feature = "client"))]
mod messages_server;
mod resources;
mod runtime;
#[cfg(feature = "client")]
mod runtime_client;
#[cfg(not(feature = "client"))]
mod runtime_server;

#[cfg(not(feature = "client"))]
mod connections;
#[cfg(not(feature = "client"))]
mod replication;

#[cfg(feature = "client")]
pub use messages::draw_server_state;
#[cfg(not(feature = "client"))]
pub use messages::on_message;
#[cfg(feature = "client")]
pub use resources::{
    GuiState, LastAckedInputSeq, LastServerState, LocalCharacterNetId, PendingReconciliation,
    ServerAddr, SinglePlayerConfig,
};
#[cfg(not(feature = "client"))]
pub use runtime::ServerSessionPlugin;
#[cfg(not(feature = "client"))]
pub use runtime::has_authority;
#[cfg(feature = "client")]
pub use runtime::{ClientSessionPlugin, cleanup_world, has_authority, snapshot_server_state};
