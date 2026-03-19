use bevy::prelude::*;

use net::message::MsgType;
use net::quic::{Channel, QuicManager, SendTarget};
use common::slow_update::SlowUpdate;

/// Smoothed network statistics updated each time a TimePong or State arrives.
#[derive(Resource, Default)]
pub struct NetworkStats {
    /// smoothed round-trip time in seconds. Zero until the first pong is received.
    pub rtt_secs: f32,
    /// Estimated ticks the client is running ahead of the server, corrected for one-way
    /// transit time (RTT/2). Positive = client ahead, negative = client behind.
    pub tick_offset: i64,
}

impl NetworkStats {
    /// One-way latency estimate in milliseconds (RTT / 2).
    // pub fn latency_ms(&self) -> f32 {
    //     self.rtt_secs * 500.0
    // }

    /// Call when a TimePong arrives to update the smoothed RTT.
    pub fn record_pong(&mut self, sent_bits: u64, now_secs: f64) {
        let sent = f64::from_bits(sent_bits);
        let rtt = (now_secs - sent).max(0.0) as f32;
        self.rtt_secs = if self.rtt_secs == 0.0 {
            rtt
        } else {
            self.rtt_secs * 0.875 + rtt * 0.125
        };
    }

    /// Call each time a State broadcast arrives to update the tick offset.
    pub fn record_state_tick(&mut self, client_tick: u64, server_tick: u64, tick_rate: f64) {
        let transit_ticks = (self.rtt_secs as f64 * tick_rate / 2.0) as i64;
        self.tick_offset = client_tick as i64 - server_tick as i64 - transit_ticks;
    }
}

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
    quic.send(
        SendTarget::All,
        Channel::Unreliable,
        &MsgType::TimePing(time.elapsed_secs_f64().to_bits()),
    );
}
