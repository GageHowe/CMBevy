use bevy::prelude::*;
use common::{slow_update::SlowUpdate, tick::NetworkStats};
use net::{
    message::MsgType,
    quic::{Channel, QuicManager},
};

pub struct TickSyncPlugin<S: States + Copy>(pub S);

impl<S: States + Copy> Plugin for TickSyncPlugin<S> {
    fn build(&self, app: &mut App) {
        let state = self.0;
        app.init_resource::<NetworkStats>()
            .add_systems(SlowUpdate, send_ping.run_if(in_state(state)));
    }
}

/// Sends a TimePing once per SlowUpdate tick (1 Hz) for RTT measurement.
fn send_ping(time: Res<Time>, mut quic: ResMut<QuicManager>) {
    quic.send_to_server(
        Channel::Unreliable,
        &MsgType::TimePing(time.elapsed_secs_f64().to_bits()),
    );
}
