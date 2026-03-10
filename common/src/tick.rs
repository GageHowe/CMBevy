use bevy::prelude::*;

/// public resource for synchronizing tick numbers for reconciliation
#[derive(Resource, Clone, Copy)]
pub struct Ticker {
    pub tick: u64
}

/// added in master plugin
pub fn increment_tick(mut tick: ResMut<Ticker>){
    tick.tick += 1;
}
