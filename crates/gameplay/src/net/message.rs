use std::{fmt::Debug, io::Read};

use bevy::{
    math::{Quat, Vec3},
    prelude::*,
};
pub use common::*;
use enum_dispatch::enum_dispatch;
use serde::*;

use crate::archetype::Archetype;
pub use crate::{projectile::ProjectileConfirmation, weapon::beamer::*};

const MAX_DECOMPRESSED_PACKET_MESSAGES_SIZE: usize = 64 * 1024 * 1024;
const ZSTD_LEVEL: i32 = 3;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JoinPlatform {
    Guest,
    Steam,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ClientHello {
    pub display_name: String,
    pub platform: JoinPlatform,
    pub proof: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct JoinAccepted {
    pub display_name: String,
    pub platform: JoinPlatform,
    pub verified_platform_user_id: Option<String>,
}

/// Type-erased spawn payload used for all replicated game objects.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnCommand {
    /// Network id the spawned object should own once it exists on the receiver.
    pub net_id: NetworkID,
    /// Optional network id of the parent entity this object should attach under.
    pub parent_net_id: Option<NetworkID>,
    pub position: Option<Vec3>,
    /// Initial world-space rotation.
    pub rotation: Option<Quat>,
    #[serde(alias = "starting_velocity")]
    pub velocity: Option<Vec3>,
    pub angular_velocity: Option<Vec3>,
    /// Authoritative server tick the spawn occurred on.
    pub server_tick: u64,
    /// Concrete game object to instantiate.
    pub archetype: Archetype,
}

impl SpawnCommand {
    pub fn new(net_id: NetworkID, archetype: Archetype, server_tick: u64) -> Self {
        Self {
            net_id,
            parent_net_id: None,
            position: None,
            rotation: None,
            velocity: None,
            angular_velocity: None,
            server_tick,
            archetype,
        }
    }

    pub fn parent(mut self, parent_net_id: NetworkID) -> Self {
        self.parent_net_id = Some(parent_net_id);
        self
    }

    pub fn position(mut self, position: Vec3) -> Self {
        self.position = Some(position);
        self
    }

    pub fn rotation(mut self, rotation: Quat) -> Self {
        self.rotation = Some(rotation);
        self
    }

    pub fn velocity(mut self, velocity: Vec3) -> Self {
        self.velocity = Some(velocity);
        self
    }

    pub fn angular_velocity(mut self, angular_velocity: Vec3) -> Self {
        self.angular_velocity = Some(angular_velocity);
        self
    }

    pub fn position_or_zero(&self) -> Vec3 {
        self.position.unwrap_or(Vec3::ZERO)
    }

    pub fn rotation_or_identity(&self) -> Quat {
        self.rotation.unwrap_or(Quat::IDENTITY)
    }

    pub fn velocity_or_zero(&self) -> Vec3 {
        self.velocity.unwrap_or(Vec3::ZERO)
    }

    pub fn angular_velocity_or_zero(&self) -> Vec3 {
        self.angular_velocity.unwrap_or(Vec3::ZERO)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum AbilityFx {
    Jetpack(bool),
    Dash(Vec3),
}

// random Messages

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Connected;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct JoinChallenge(pub String);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct JoinRejected(pub String);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ClientReady;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestMap;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Disconnected;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ChatMessage(pub Color, pub String);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Ping(pub String);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Pong(pub String);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Input {
    pub seq: u64,
    pub input: PawnInput,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct State(pub SimulationState);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DespawnCommand(pub NetworkID);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Possess(pub NetworkID);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct MountState {
    pub biped_net_id: NetworkID,
    pub parent_net_id: Option<NetworkID>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Interact(pub NetworkID);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DropWeapon(pub Vec3);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DropAbility(pub Vec3);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SetActiveWeaponSlot(pub bool);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WeaponPickup {
    pub weapon_id: NetworkID,
    pub carrier_net_id: NetworkID,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WeaponDrop {
    pub weapon_id: NetworkID,
    pub carrier_net_id: NetworkID,
    pub drop_pos: Vec3,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct BipedLook {
    pub net_id: NetworkID,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct MeleeHitRequest(pub NetworkID);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ProjectileSpawn {
    pub weapon: NetworkID,
    pub net_id: NetworkID,
    pub position: Vec3,
    pub starting_velocity: Vec3,
    pub shooter_velocity: Vec3,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct HitResult {
    pub position: Vec3,
    pub normal: Vec3,
    pub target: Option<NetworkID>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct TimePing(pub u64);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct TimePong(pub u64);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct AbilityFxMessage {
    pub net_id: NetworkID,
    pub fx: AbilityFx,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct AbilityState {
    pub owner_net_id: NetworkID,
    pub ability: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WeaponStateMessage {
    pub net_id: NetworkID,
    pub weapon_state: WeaponState,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Health {
    pub net_id: NetworkID,
    pub current: i32,
    pub max: i32,
    pub regen_per_tick_num: i32,
    pub regen_delay_ticks: u16,
    pub regen_delay_remaining_ticks: u16,
    pub regen_accum: i32,
    pub damage_accum_millis: i32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct MapHash(pub String);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct FileData {
    pub name: String,
    pub compressed: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[enum_dispatch(Message)]
/// Transport-level message enum shared by client and server.
pub enum MsgType {
    Connected(Connected),
    JoinChallenge(JoinChallenge),
    ClientHello(ClientHello),
    JoinAccepted(JoinAccepted),
    JoinRejected(JoinRejected),
    ClientReady(ClientReady),
    RequestMap(RequestMap),
    Disconnected(Disconnected),
    ChatMessage(ChatMessage),
    Ping(Ping),
    Pong(Pong),
    Input(Input),
    /// map of NetworkID to rigidbody state
    State(State),
    SpawnCommand(SpawnCommand),
    DespawnCommand(DespawnCommand),
    Possess(Possess),
    MountState(MountState),
    Interact(Interact),
    DropWeapon(DropWeapon),
    DropAbility(DropAbility),
    SetActiveWeaponSlot(SetActiveWeaponSlot),
    WeaponPickup(WeaponPickup),
    WeaponDrop(WeaponDrop),
    /// client-authoritative: "I am looking with this yaw and pitch"
    BipedLook(BipedLook),
    MeleeHitRequest(MeleeHitRequest),
    ProjectileSpawn(ProjectileSpawn),
    StartBeamCharge(StartBeamCharge),
    StartBeam(StartBeam),
    BeamHitReport(BeamHitReport),
    EndBeam(EndBeam),
    ProjectileConfirm(ProjectileConfirmation),
    HitResult(HitResult),
    TimePing(TimePing),
    TimePong(TimePong),
    AbilityFx(AbilityFxMessage),
    AbilityState(AbilityState),
    WeaponState(WeaponStateMessage),
    Health(Health),
    MapHash(MapHash),
    FileData(FileData),
}

// ----------- IT'S BEAUTIFUL

/// trait all networked message types are required to implement
#[enum_dispatch]
pub trait Message: Serialize + serde::de::DeserializeOwned + Debug + PartialEq + Clone {
    /// mutate the world in some way in response to receiving this message
    fn handle(self, world: &mut World);
}

// ----------- INDIVIDUAL PACKET TYPES ------------

macro_rules! impl_noop_message {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Message for $ty {
                fn handle(self, _world: &mut World) {}
            }
        )*
    }
}

impl_noop_message!(
    Connected,
    JoinChallenge,
    ClientHello,
    JoinAccepted,
    JoinRejected,
    ClientReady,
    RequestMap,
    Disconnected,
    Ping,
    Pong,
    Input,
    State,
    DespawnCommand,
    Possess,
    MountState,
    Interact,
    DropWeapon,
    DropAbility,
    SetActiveWeaponSlot,
    BipedLook,
    MeleeHitRequest,
    ProjectileSpawn,
    HitResult,
    TimePing,
    TimePong,
    AbilityFxMessage,
    AbilityState,
    WeaponStateMessage,
    Health,
);

impl Message for ChatMessage {
    fn handle(self, world: &mut World) {
        #[cfg(feature = "client")]
        if let Some(mut state) = world.get_resource_mut::<crate::session::GuiState>() {
            state.chat.push(self);
            let excess = state.chat.len().saturating_sub(200);
            let _ = state.chat.drain(..excess);
        }

        #[cfg(not(feature = "client"))]
        if let Some(mut quic) = world.get_resource_mut::<crate::net::quic::QuicManager>() {
            quic.send(
                crate::net::quic::SendTarget::All,
                crate::net::quic::Channel::Ordered,
                &MsgType::ChatMessage(self),
            );
        }
    }
}

#[cfg(not(feature = "client"))]
impl_noop_message!(MapHash, FileData);

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Packet {
    #[serde(
        serialize_with = "serialize_then_compress",
        deserialize_with = "decompress_then_deserialize"
    )]
    messages: Vec<MsgType>,
    frame_number: u64,
}
impl Packet {
    pub fn new(messages: Vec<MsgType>, frame_number: u64) -> Self {
        Self {
            messages,
            frame_number,
        }
    }

    pub fn handle_all(self, world: &mut World) {
        for msg in self.messages {
            msg.handle(world);
        }
    }
}

// ---------- SERDE FOR Packet -----------

fn serialize_then_compress<S>(messages: &Vec<MsgType>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let serialized = postcard::to_allocvec(messages).map_err(serde::ser::Error::custom)?;
    let compressed = zstd::stream::encode_all(serialized.as_slice(), ZSTD_LEVEL)
        .map_err(serde::ser::Error::custom)?;

    compressed.serialize(serializer)
}

fn decompress_then_deserialize<'de, D>(deserializer: D) -> Result<Vec<MsgType>, D::Error>
where
    D: Deserializer<'de>,
{
    let compressed = Vec::<u8>::deserialize(deserializer)?;
    let mut decoder = zstd::stream::read::Decoder::new(compressed.as_slice())
        .map_err(serde::de::Error::custom)?;
    let mut serialized = Vec::new();

    decoder
        .by_ref()
        .take((MAX_DECOMPRESSED_PACKET_MESSAGES_SIZE + 1) as u64)
        .read_to_end(&mut serialized)
        .map_err(serde::de::Error::custom)?;

    if serialized.len() > MAX_DECOMPRESSED_PACKET_MESSAGES_SIZE {
        return Err(serde::de::Error::custom(
            "decompressed packet messages exceed the size limit",
        ));
    }

    postcard::from_bytes(&serialized).map_err(serde::de::Error::custom)
}
