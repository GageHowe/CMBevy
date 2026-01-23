// use bevy_renet::*;

// this module will contain information about packets, synchronization, etc
use bevy::prelude::*;
use bevy_renet::renet::*;
use bevy_renet::*;
use serde::{Deserialize, Serialize};

// makes sure clients are on the same version
pub const VERSION: u64 = 1000;

// https://www.youtube.com/watch?v=fBHO0yptg1Y

#[derive(Debug, Serialize, Deserialize)]
pub enum Ping {
    PingPing,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Pong {
    PongPong,
}
