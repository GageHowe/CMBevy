use std::collections::HashMap;

use bevy::prelude::*;
#[cfg(not(feature = "client"))]
use game_objects::NetworkEntityMap;
#[cfg(not(feature = "client"))]
use game_objects::level::SpawnPoint;
#[cfg(feature = "client")]
use game_objects::projectile::{PredictedProjectileMap, ProjectileState};
#[cfg(not(feature = "client"))]
use game_objects::weapon::{WeaponConfig, WeaponState};
use net::message::{NetworkID, SimulationState};
#[cfg(not(feature = "client"))]
use net::quic::ConnectionId;
use ::pawn as pawn_crate;
#[cfg(not(feature = "client"))]
use ::pawn::VehicleComponent;
#[cfg(feature = "client")]
use ::pawn::WeaponSlots;
#[cfg(feature = "client")]
use ::pawn::biped::BipedPawnComponent;
#[cfg(not(feature = "client"))]
use ::pawn::{BipedPawnComponent, CharacterMount, HeldWeaponMap, Mounted, PawnInputKind, WeaponSlots};
#[cfg(feature = "client")]
use ::pawn::{CharacterMount, Mounted, Possessed};

#[cfg(feature = "client")]
#[derive(Resource, Clone)]
/// Selected multiplayer server address for the client runtime.
pub struct ServerAddr {
    pub addr: std::net::SocketAddr,
    pub lobby_id: Option<String>,
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Requests a clean client exit back to the shell.
pub struct PendingExit(pub bool);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Last authoritative physics snapshot received from the server.
pub struct LastServerState(pub Option<SimulationState>);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Highest input sequence acknowledged by the authoritative server.
pub struct LastAckedInputSeq(pub u64);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Stable local player character id used by UI that should not follow vehicle/turret possession.
pub struct LocalCharacterNetId(pub Option<NetworkID>);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Gates the final "world ready" acknowledgement until map load finishes.
pub struct PendingWorldReady(pub bool);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Snapshot waiting to be applied by the reconciliation system.
pub struct PendingReconciliation(pub Option<SimulationState>);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Small bag of UI/runtime state owned by the client session layer.
pub struct GuiState {
    pub command_input: String,
    pub log: Vec<String>,
    pub scoreboard: Option<net::message::ScoreboardSnapshot>,
}

#[cfg(feature = "client")]
impl GuiState {
    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Marker resource for menu/runtime hosted-server actions.
pub struct HostedServer;

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Single-player session configuration selected from the menu.
pub struct SinglePlayerConfig {
    pub map: String,
    pub gametype: String,
    pub(crate) timer: Option<f32>,
    pub(crate) spawned_once: bool,
}

#[cfg(feature = "client")]
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct SpawnParams<'w, 's> {
    pub commands: Commands<'w, 's>,
}

#[cfg(feature = "client")]
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ClientMessageParams<'w, 's> {
    pub spawn: SpawnParams<'w, 's>,
    pub world: ResMut<'w, physics::physics_world::PhysicsWorld>,
    pub biped_q: ParamSet<
        'w,
        's,
        (
            Query<'w, 's, (&'static mut WeaponSlots, &'static BipedPawnComponent), With<Possessed>>,
            Query<'w, 's, &'static BipedPawnComponent>,
            Query<'w, 's, &'static mut BipedPawnComponent>,
        ),
    >,
    pub networked: Res<'w, game_objects::NetworkEntityMap>,
    pub camera: Query<'w, 's, Entity, With<Camera3d>>,
    pub projectile_q: Query<'w, 's, (Entity, &'static ProjectileState)>,
    pub predicted_projectiles: ResMut<'w, PredictedProjectileMap>,
    pub interaction_names: Query<'w, 's, &'static game_objects::interaction::InteractionName>,
    pub pickup_fns: Query<'w, 's, &'static pawn_crate::biped_ability::OnPickup>,
    pub mounted: Query<'w, 's, &'static Mounted>,
    pub mounts: Query<'w, 's, &'static CharacterMount>,
    pub mount_anchor_transforms: Query<'w, 's, &'static Transform>,
    pub pending_weapon_pickups: ResMut<'w, PendingWeaponPickups>,
}

#[cfg(feature = "client")]
pub(crate) type JustSpawned = HashMap<NetworkID, (Entity, u64)>;

#[cfg(feature = "client")]
#[derive(Resource, Default)]
pub(crate) struct PendingWeaponPickups(pub Vec<(NetworkID, NetworkID)>);

#[cfg(not(feature = "client"))]
#[derive(Resource)]
pub(crate) struct BindAddr(pub std::net::SocketAddr);

#[cfg(not(feature = "client"))]
#[derive(Resource)]
pub(crate) struct ConsoleCommands(pub std::sync::Mutex<std::sync::mpsc::Receiver<String>>);

#[cfg(not(feature = "client"))]
#[derive(Resource)]
pub(crate) struct LevelPath(pub String);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub(crate) struct BodyHistory(pub HashMap<u64, SimulationState>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
/// Latest input per connection waiting to be applied on the server tick.
pub struct PendingInputs(pub HashMap<ConnectionId, (u64, PawnInputKind)>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub struct PendingMeleeHits(pub HashMap<ConnectionId, NetworkID>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub(crate) struct LastProcessedInputSeq(pub HashMap<ConnectionId, u64>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
/// Connections that have completed transport setup and are ready for initial world sync.
pub struct PendingConnections(pub std::collections::HashSet<ConnectionId>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
/// Connections currently considered active by the session layer.
pub struct ActiveConnections(pub std::collections::HashSet<ConnectionId>);

#[cfg(not(feature = "client"))]
#[derive(bevy::ecs::system::SystemParam)]
pub struct ServerMessageParams<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub world: ResMut<'w, physics::physics_world::PhysicsWorld>,
    pub held_weapons: ResMut<'w, HeldWeaponMap>,
    pub spawn_points: Query<
        'w,
        's,
        (
            Entity,
            &'static SpawnPoint,
            &'static Transform,
            Option<&'static ChildOf>,
        ),
    >,
    pub parent_transforms: Query<'w, 's, &'static Transform>,
    pub parent_parents: Query<'w, 's, &'static ChildOf>,
    pub parent_bodies: Query<'w, 's, &'static physics::physics_world::RigidBodyHandleComponent>,
    pub level_ready: game_objects::level::LevelReadyState<'w, 's>,
    pub spawnables: Query<
        'w,
        's,
        (
            Entity,
            &'static NetworkID,
            Option<&'static physics::physics_world::RigidBodyHandleComponent>,
            Option<&'static ChildOf>,
            Option<&'static Transform>,
        ),
    >,
    pub all_networked: Res<'w, NetworkEntityMap>,
    pub pawn_slots: Query<'w, 's, &'static mut WeaponSlots>,
    pub bipeds: Query<'w, 's, &'static mut BipedPawnComponent>,
    pub vehicles: Query<'w, 's, &'static VehicleComponent>,
    pub interactables: Query<'w, 's, &'static game_objects::interaction::Interactable>,
    pub net_ids: Query<'w, 's, &'static NetworkID>,
    pub mounted_bipeds: Query<'w, 's, (&'static NetworkID, &'static Mounted)>,
    pub mounts: Query<'w, 's, &'static mut CharacterMount>,
    pub mount_anchor_transforms: Query<'w, 's, &'static Transform>,
    pub weapon_runtime: Query<'w, 's, (&'static mut WeaponState, &'static WeaponConfig)>,
    pub beamers: Query<'w, 's, &'static mut game_objects::weapon::beamer::BeamerComponent>,
    pub health_q: Query<'w, 's, &'static mut game_objects::health::Health>,
    pub last_damage_q: Query<'w, 's, &'static mut game_objects::health::LastDamageSource>,
    pub on_pickup_q: Query<'w, 's, &'static pawn_crate::biped_ability::OnPickup>,
    pub biped_spawnables: Query<'w, 's, (), With<pawn_crate::BipedPawnComponent>>,
    pub spaceship_spawnables: Query<'w, 's, (), With<pawn_crate::SpaceshipPawnComponent>>,
    pub fighter_spawnables: Query<'w, 's, (), With<pawn_crate::FighterPawnComponent>>,
    pub truck_spawnables: Query<'w, 's, (), With<pawn_crate::TruckPawnComponent>>,
    pub hovercraft_spawnables: Query<'w, 's, (), With<pawn_crate::HovercraftPawnComponent>>,
    pub shield_spawnables: Query<'w, 's, (), With<game_objects::shield::Shield>>,
    pub pistol_spawnables: Query<'w, 's, (), With<game_objects::weapon::pistol::PistolComponent>>,
    pub beamer_spawnables: Query<'w, 's, (), With<game_objects::weapon::beamer::BeamerComponent>>,
    pub rifle_spawnables: Query<'w, 's, (), With<game_objects::weapon::rifle::RifleComponent>>,
    pub smg_spawnables: Query<'w, 's, (), With<game_objects::weapon::smg::SmgComponent>>,
    pub hail_mary_spawnables:
        Query<'w, 's, (), With<game_objects::weapon::hail_mary::HailMaryComponent>>,
    pub thumper_spawnables:
        Query<'w, 's, (), With<game_objects::weapon::thumper::ThumperComponent>>,
    pub lobber_spawnables: Query<'w, 's, (), With<game_objects::weapon::lobber::LobberComponent>>,
    pub coil_launcher_spawnables:
        Query<'w, 's, (), With<game_objects::weapon::coil_launcher::CoilLauncherComponent>>,
    pub interaction_name_spawnables:
        Query<'w, 's, &'static game_objects::interaction::InteractionName>,
}
