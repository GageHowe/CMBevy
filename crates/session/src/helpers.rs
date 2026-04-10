use game_objects::NetworkEntityMap;
use net::message::NetworkID;
use net::quic::{InboundMessage, QuicManager};

pub(crate) fn find_networked_entity(
    all_networked: &NetworkEntityMap,
    net_id: &NetworkID,
) -> Option<bevy::prelude::Entity> {
    all_networked.get(net_id)
}

pub(crate) fn drain_inbound(
    quic: &mut QuicManager,
    mut handle: impl FnMut(InboundMessage, &mut QuicManager),
) {
    while let Some(msg) = quic.inbound.pop_front() {
        handle(msg, quic);
    }
}
