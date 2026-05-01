use net::quic::{InboundMessage, QuicManager};

pub(crate) fn drain_inbound(
    quic: &mut QuicManager,
    mut handle: impl FnMut(InboundMessage, &mut QuicManager),
) {
    while let Some(msg) = quic.inbound.pop_front() {
        handle(msg, quic);
    }
}
