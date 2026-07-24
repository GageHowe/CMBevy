mod resources;
mod runtime;
#[cfg(feature = "client")]
mod runtime_client;
#[cfg(not(feature = "client"))]
mod runtime_server;

#[cfg(not(feature = "client"))]
mod replication;

use bevy::prelude::World;
#[cfg(feature = "client")]
pub use resources::{
    GuiState, LastAckedInputSeq, LocalCharacterNetId, PendingReconciliation, ServerAddr,
    SinglePlayerConfig,
};
#[cfg(not(feature = "client"))]
pub use runtime::ServerSessionPlugin;
#[cfg(not(feature = "client"))]
pub use runtime::has_authority;
#[cfg(feature = "client")]
pub use runtime::{ClientSessionPlugin, cleanup_world, has_authority};

use crate::net::quic::QuicManager;

/// calls per-message handlers of received messages
pub fn on_message(world: &mut World) {
    let packets = {
        let Some(mut quic) = world.get_resource_mut::<QuicManager>() else {
            return;
        };
        quic.inbound
            .drain(..)
            .map(|inbound| inbound.packet)
            .collect::<Vec<_>>()
    };

    for packet in packets {
        packet.handle_all(world); // calls out to enum_dispatch and Message trait
    }
}
