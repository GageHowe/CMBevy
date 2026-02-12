use crate::types::{CMQuat, CMVec3};
use bevy::prelude::*;
use std::collections::HashMap;
use std::io;
use std::io::Cursor;
use std::{error::Error, net::SocketAddr, sync::Arc};
use wincode::serialize;
use wincode_derive::{SchemaRead, SchemaWrite};
use zstd::{decode_all, encode_all};

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub enum MsgType {
    /// address, message
    ChatMessage(String, String),
    // HitReport()
    /// A message the recipient will display in messagebar
    Error(String),

    BodyState(BodyState),
    State(SimulationState),
}

/// Component to mark entities that should be networked.
/// NetworkID is managed by the server.
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Eq, Clone, Component, Hash)]
pub struct NetworkID(pub u32);

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub struct BodyState {
    pub position: CMVec3,
    pub rotation: CMQuat,
    pub linvel: CMVec3,
    pub angvel: CMVec3,
}

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub struct SimulationState {
    pub tick: u64,
    pub bodies: HashMap<NetworkID, BodyState>,
}

// fn request_pawn() -> pawn type, NetworkID
