use crate::types::{CMQuat, CMVec3};
use bevy::prelude::*;
use std::collections::HashMap;
use wincode_derive::{SchemaRead, SchemaWrite};
use std::str::FromStr;


#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub enum MsgType {
    /// sender, message
    ChatMessage(String, String),
    // HitReport()
    /// A message the recipient will display in messagebar
    Error(String),
    Ping(String),
    Pong(String),

    BodyState(BodyState),
    State(SimulationState),
}
impl FromStr for MsgType {
    type Err = String;

    /// try to recognize a user-provided command for runtime debugging purposes
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (cmd, rest) = s.split_once(' ').unwrap_or((s, ""));

        match cmd {
            "chat" => {
                let (sender, msg) = rest.split_once(' ')
                    .ok_or("usage: chat <sender> <message>")?;
                Ok(MsgType::ChatMessage(sender.to_string(), msg.to_string()))
            }
            "error" => Ok(MsgType::Error(rest.to_string())),
            "ping" => Ok(MsgType::Ping(rest.to_string())),
            "pong" => Ok(MsgType::Pong(rest.to_string())),
            _ => Err(format!("unknown command: {cmd}")),
        }
        /*
        usage:
            let msg: MsgType = "ping hello".parse()?;
            let msg = MsgType::from_str("chat alice hello world")?;
         */
    }
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
