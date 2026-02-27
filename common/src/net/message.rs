use crate::types::{CMQuat, CMVec3};
use bevy::prelude::*;
use std::collections::HashMap;
use wincode_derive::{SchemaRead, SchemaWrite};
use std::str::FromStr;

/// Component to mark entities that should be networked.
/// NetworkID is managed by the server.
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Eq, Clone, Component, Hash)]
pub struct NetworkID(pub u64);

#[derive(Resource, Default)]
pub struct NetworkIDResource {
    pub last_id: u64
}
impl NetworkIDResource {
    /// Should be used when spawning a new networked entity
    pub fn get_next_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }
}

/// Object types the server can tell clients to spawn
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub enum ObjectType {
    Biped,
    Spaceship,
    Projectile1,
    Projectile2,
    // BulletCasing, // local-only projectile, probably should not be networked
    Bergentruck, // beer!
}

/// Server tells clients to spawn an object
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub struct SpawnCommand {
    pub net_id: NetworkID,
    pub kind: ObjectType,
    pub location: Option<CMVec3>,
    /// velocity to start at
    pub velocity: Option<CMVec3>,
    // inherit_velocity: bool, // nvm, simply add velocity on server side
    pub rotation: Option<CMQuat>,
}

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
    SpawnCommand(SpawnCommand)
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
