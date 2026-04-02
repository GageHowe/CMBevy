use bevy::prelude::*;

/// public resource for synchronizing tick numbers for reconciliation
#[derive(Resource, Clone, Copy)]
pub struct Ticker {
    pub tick: u64,
}

#[derive(Resource, Default)]
pub struct NetworkStats {
    pub rtt_secs: f32,
}

impl NetworkStats {
    pub fn record_pong(&mut self, sent_bits: u64, now_secs: f64) {
        let sent = f64::from_bits(sent_bits);
        let rtt = (now_secs - sent).max(0.0) as f32;
        self.rtt_secs = if self.rtt_secs == 0.0 {
            rtt
        } else {
            self.rtt_secs * 0.875 + rtt * 0.125
        };
    }
}

/// added in master plugin
pub fn increment_tick(mut tick: ResMut<Ticker>) {
    tick.tick += 1;
}
