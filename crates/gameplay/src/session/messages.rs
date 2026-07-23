use bevy::prelude::*;

use crate::net::quic::QuicManager;

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
        packet.handle_all(world);
    }
}
