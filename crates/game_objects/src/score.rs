use bevy::prelude::*;
use net::quic::ConnectionId;
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct PlayerScores(pub HashMap<ConnectionId, i32>);
