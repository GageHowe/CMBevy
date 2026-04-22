// pub mod kinds;
pub mod biped;
pub mod biped_ability;
pub mod spaceship;
pub mod vehicle;
pub mod weapon_slots;

#[cfg(feature = "client")]
use noise_functions::{Noise, Perlin};

/// spring-damping recoil + procedural shake + zoom applied on top of gameplay aim.
/// Placed on the Camera3d entity by biped possession; any pawn system can write to it.
#[derive(bevy::prelude::Component)]
pub struct CameraEffector {
    pub pitch_offset: f32,
    pub pitch_vel: f32,
    pub yaw_offset: f32,
    pub yaw_vel: f32,
    pub base_translation: Vec3,
    /// for recoil recovery
    pub recovery_speed: f32,
    /// User's base FOV in degrees. Set at possession; updated when settings change.
    pub base_fov: f32,
    /// Zoom multiplier for this tick (1.0 = no zoom). Weapons write this; no auto-reset.
    pub zoom_multiplier: f32,
    /// Smoothly lerped FOV in degrees, written to Projection each frame.
    pub current_fov: f32,
    #[cfg(feature = "client")]
    active_shakes: Vec<ActiveCameraShake>,
}
impl Default for CameraEffector {
    fn default() -> Self {
        Self {
            pitch_offset: 0.0,
            pitch_vel: 0.0,
            yaw_offset: 0.0,
            yaw_vel: 0.0,
            base_translation: Vec3::ZERO,
            recovery_speed: 18.0,
            base_fov: 90.0,
            zoom_multiplier: 1.0,
            current_fov: 90.0,
            #[cfg(feature = "client")]
            active_shakes: Vec::new(),
        }
    }
}
impl CameraEffector {
    pub fn add_kick(&mut self, vertical: (f32, f32), horizontal: (f32, f32), recovery_speed: f32) {
        self.pitch_vel += vertical.0 + fastrand::f32() * (vertical.1 - vertical.0);
        self.yaw_vel += horizontal.0 + fastrand::f32() * (horizontal.1 - horizontal.0);
        self.recovery_speed = recovery_speed;
    }
    #[cfg(feature = "client")]
    pub fn add_shake(&mut self, shake: CameraShake) {
        if shake.duration <= 0.0
            || shake.frequency <= 0.0
            || (shake.translation == Vec3::ZERO
                && shake.rotation == Vec2::ZERO
                && shake.roll == 0.0)
        {
            return;
        }
        self.active_shakes.push(ActiveCameraShake { shake, age: 0.0, seed: fastrand::i32(..) });
    }
    pub fn current_zoom_factor(&self) -> f32 {
        let base = (self.base_fov.to_radians() * 0.5).tan();
        let current = (self.current_fov.to_radians() * 0.5).tan();
        if current > 0.0 { (base / current).max(1.0) } else { 1.0 }
    }
    pub fn reset_zoom(&mut self) {
        self.zoom_multiplier = 1.0;
        self.current_fov = self.base_fov;
    }

    #[cfg(feature = "client")]
    fn sample_shakes(&mut self, dt: f32) -> (Vec3, Vec2, f32) {
        let mut translation = Vec3::ZERO;
        let mut rotation = Vec2::ZERO;
        let mut roll = 0.0;
        self.active_shakes.retain_mut(|active| {
            active.age += dt;
            let life = (active.age / active.shake.duration).clamp(0.0, 1.0);
            let envelope = (1.0 - life) * (1.0 - life);
            if envelope <= 0.0 {
                return false;
            }
            let sample_t = active.age * active.shake.frequency;
            translation.x +=
                active.shake.translation.x * envelope * perlin_1d(sample_t, active.seed, 11.0);
            translation.y +=
                active.shake.translation.y * envelope * perlin_1d(sample_t, active.seed, 23.0);
            translation.z +=
                active.shake.translation.z * envelope * perlin_1d(sample_t, active.seed, 37.0);
            rotation.x +=
                active.shake.rotation.x * envelope * perlin_1d(sample_t, active.seed, 41.0);
            rotation.y +=
                active.shake.rotation.y * envelope * perlin_1d(sample_t, active.seed, 53.0);
            roll += active.shake.roll * envelope * perlin_1d(sample_t, active.seed, 67.0);
            true
        });
        (translation, rotation, roll)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CameraShake {
    pub translation: Vec3,
    pub rotation: Vec2,
    pub roll: f32,
    pub duration: f32,
    pub frequency: f32,
}

impl CameraShake {
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            translation: self.translation * scale,
            rotation: self.rotation * scale,
            roll: self.roll * scale,
            ..self
        }
    }
}

#[cfg(feature = "client")]
struct ActiveCameraShake {
    shake: CameraShake,
    age: f32,
    seed: i32,
}
// pub mod dep;

use std::collections::HashMap;

use bevy::prelude::*;
pub use biped::{BipedPawnComponent, PitchPivot, YawPivot};
use common::GameObjectKind;
#[cfg(feature = "client")]
use common::PredictedCommands;
pub use common::{BipedInput, PawnInputKind, SpaceshipInput};
#[cfg(feature = "client")]
use net::message::MsgType;
use net::{
    message::NetworkID,
    quic::{Channel, ConnectionId, QuicManager, SendTarget},
};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
pub use spaceship::SpaceshipPawnComponent;
pub use vehicle::{SeatedInVehicle, VehicleComponent};
pub use weapon_slots::WeaponSlots;

use crate::GameObject;

/// Tracks two different pawn identities per connection.
///
/// `controlled_by_conn` is the pawn currently receiving that client's inputs.
/// This can be a vehicle while the player is driving.
///
/// `character_by_conn` is the player's persistent biped character.
/// Interactions, inventory, respawns, and other character-owned state should use this.
#[derive(Resource, Default)]
pub struct PlayerRegistry {
    controlled_by_conn: HashMap<ConnectionId, (Entity, NetworkID)>,
    character_by_conn: HashMap<ConnectionId, (Entity, NetworkID)>,
    conn_by_character_entity: HashMap<Entity, ConnectionId>,
}
impl PlayerRegistry {
    pub fn register_character(&mut self, conn_id: ConnectionId, entity: Entity, net_id: NetworkID) {
        self.controlled_by_conn.insert(conn_id, (entity, net_id.clone()));
        if let Some((old_entity, _)) =
            self.character_by_conn.insert(conn_id, (entity, net_id.clone()))
        {
            self.conn_by_character_entity.remove(&old_entity);
        }
        self.conn_by_character_entity.insert(entity, conn_id);
    }

    pub fn set_controlled_pawn(
        &mut self,
        conn_id: ConnectionId,
        entity: Entity,
        net_id: NetworkID,
    ) {
        self.controlled_by_conn.insert(conn_id, (entity, net_id));
    }

    pub fn controlled_pawn(&self, conn_id: ConnectionId) -> Option<(Entity, &NetworkID)> {
        self.controlled_by_conn.get(&conn_id).map(|(entity, net_id)| (*entity, net_id))
    }

    pub fn character(&self, conn_id: ConnectionId) -> Option<(Entity, &NetworkID)> {
        self.character_by_conn.get(&conn_id).map(|(entity, net_id)| (*entity, net_id))
    }

    pub fn remove_character_for_conn(
        &mut self,
        conn_id: ConnectionId,
    ) -> Option<(Entity, NetworkID)> {
        self.controlled_by_conn.remove(&conn_id);
        let (entity, net_id) = self.character_by_conn.remove(&conn_id)?;
        self.conn_by_character_entity.remove(&entity);
        Some((entity, net_id))
    }

    pub fn remove_character(&mut self, entity: Entity) -> Option<(ConnectionId, NetworkID)> {
        let conn_id = self.conn_by_character_entity.remove(&entity)?;
        self.controlled_by_conn.remove(&conn_id);
        let (_, net_id) = self.character_by_conn.remove(&conn_id)?;
        Some((conn_id, net_id))
    }

    pub fn conn_id_for_character(&self, entity: Entity) -> Option<ConnectionId> {
        self.conn_by_character_entity.get(&entity).copied()
    }

    pub fn controlled_count(&self) -> usize {
        self.controlled_by_conn.len()
    }

    pub fn controlled_conn_ids(&self) -> impl Iterator<Item = ConnectionId> + '_ {
        self.controlled_by_conn.keys().copied()
    }

    pub fn controlled_entries(
        &self,
    ) -> impl Iterator<Item = (&ConnectionId, &(Entity, NetworkID))> + '_ {
        self.controlled_by_conn.iter()
    }

    pub fn character_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.character_by_conn.values().map(|(entity, _)| *entity)
    }
}

/// Switches the input-controlled pawn for a connection and tells that client to possess it.
pub fn possess_pawn(
    conn_id: ConnectionId,
    entity: Entity,
    net_id: &NetworkID,
    registry: &mut PlayerRegistry,
    quic: &mut QuicManager,
) {
    registry.set_controlled_pawn(conn_id, entity, net_id.clone());
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &net::message::MsgType::Possess(net_id.clone()),
    );
}

/// Broadcasts whether a character is seated in a vehicle.
pub fn broadcast_seat_state(
    quic: &mut QuicManager,
    biped_net_id: &NetworkID,
    vehicle_net_id: Option<&NetworkID>,
) {
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &net::message::MsgType::SeatState(biped_net_id.clone(), vehicle_net_id.cloned()),
    );
}

/// Pending respawns: conn_id -> (seconds_remaining, kind).
#[derive(Resource, Default)]
pub struct PendingRespawns(pub HashMap<ConnectionId, (f32, GameObjectKind, crate::Team)>);

#[derive(Resource, Default)]
pub struct HeldWeaponMap(pub HashMap<NetworkID, Entity>);

pub struct PawnPlugin;
impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LookSnapCompensation>();
        #[cfg(feature = "client")]
        app.init_resource::<InteractionGate>().init_resource::<InteractionHint>().add_systems(
            PostUpdate,
            apply_camera_effects.before(bevy::transform::TransformSystems::Propagate),
        );
        app.add_plugins(biped_ability::BipedAbilityPlugin);
        app.add_plugins(biped::BipedPlugin);
        app.add_plugins(spaceship::SpaceshipPlugin);
        app.add_plugins(vehicle::VehiclePlugin);
    }
}

#[derive(Resource, Clone, Copy)]
pub struct LookSnapCompensation(pub bool);
impl Default for LookSnapCompensation {
    fn default() -> Self {
        Self(true)
    }
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
pub struct InteractionGate {
    pressed: bool,
    next_tick: u64,
}

#[cfg(feature = "client")]
impl InteractionGate {
    const COOLDOWN_TICKS: u64 = 12;

    pub fn consume_press(&mut self, is_down: bool, tick: u64) -> bool {
        if !is_down {
            self.pressed = false;
            return false;
        }
        if self.pressed || tick < self.next_tick {
            return false;
        }
        self.pressed = true;
        self.next_tick = tick + Self::COOLDOWN_TICKS;
        true
    }
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
pub struct InteractionHint(pub Option<String>);

/// all pawns implement this; defines input and movement
pub trait Pawn: Component<Mutability = bevy::ecs::component::Mutable> + GameObject {
    fn apply_input(
        &mut self,
        world: &mut PhysicsWorld,
        body: &RigidBodyHandleComponent,
        input: PawnInputKind,
    );
}

#[cfg(not(feature = "client"))]
pub fn apply_server_input(
    entity: Entity,
    input: PawnInputKind,
    world: &mut PhysicsWorld,
    bipeds: &mut Query<&mut biped::BipedPawnComponent>,
    spaceships: &mut Query<&mut spaceship::SpaceshipPawnComponent>,
) -> (bool, Option<biped_ability::AbilityFx>) {
    let Some(handle) = world.entity_to_handle.get(&entity).copied() else {
        return (false, None);
    };
    match input {
        PawnInputKind::Biped(input) => {
            let Ok(mut biped) = bipeds.get_mut(entity) else {
                return (false, None);
            };
            let fx = biped::apply_biped_input(
                world,
                entity,
                input,
                &RigidBodyHandleComponent(handle),
                &mut biped,
            );
            (true, fx)
        }
        PawnInputKind::Spaceship(input) => {
            let Ok(mut ship) = spaceships.get_mut(entity) else {
                return (false, None);
            };
            spaceship::apply_spaceship_movement(
                world,
                &RigidBodyHandleComponent(handle),
                input,
                &mut ship,
            );
            (true, None)
        }
    }
}

// CAMERA

/// runtime mouse sensitivity, set from the Settings resource by SettingsPlugin. Only needed by client.
#[derive(Resource)]
pub struct MouseSensitivity {
    pub base: f32,
    pub zoom_blend: f32,
    pub vehicle_pitch_yaw: f32,
}

#[cfg(feature = "client")]
const KICK_DAMPING: f32 = 0.88; // velocity multiplier per tick at 60 Hz
#[cfg(feature = "client")]
const FOV_LERP_SPEED: f32 = 15.0; // how fast zoom eases in/out

/// Integrates recoil, procedural shake, and FOV zoom. Writes Camera3d local Transform and Projection.
#[cfg(feature = "client")]
fn apply_camera_effects(
    time: Res<Time>,
    mut camera_q: Query<(&mut Transform, &mut CameraEffector, &mut Projection), With<Camera3d>>,
) {
    let Ok((mut transform, mut fx, mut proj)) = camera_q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    let damp = KICK_DAMPING.powf(dt * 60.0);
    let decay = (-fx.recovery_speed * dt).exp();
    fx.pitch_vel *= damp;
    fx.pitch_offset = (fx.pitch_offset + fx.pitch_vel * dt) * decay;
    fx.yaw_vel *= damp;
    fx.yaw_offset = (fx.yaw_offset + fx.yaw_vel * dt) * decay;

    let (shake_translation, shake_rotation, shake_roll) = fx.sample_shakes(dt);
    transform.translation = fx.base_translation + shake_translation;
    transform.rotation = Quat::from_euler(
        EulerRot::XYZ,
        fx.pitch_offset + shake_rotation.x,
        fx.yaw_offset + shake_rotation.y,
        shake_roll,
    );

    let target_fov =
        ((fx.base_fov / 2.0).to_radians().tan() / fx.zoom_multiplier).atan().to_degrees() * 2.0;
    fx.current_fov += (target_fov - fx.current_fov) * (1.0 - (-FOV_LERP_SPEED * dt).exp());
    if let Projection::Perspective(ref mut p) = *proj {
        p.fov = fx.current_fov.to_radians();
    }
}

#[cfg(feature = "client")]
fn perlin_1d(x: f32, seed: i32, channel: f32) -> f32 {
    Perlin.seed(seed).sample2([x, channel]) as f32
}
impl Default for MouseSensitivity {
    fn default() -> Self {
        Self { base: 0.002, zoom_blend: 1.0, vehicle_pitch_yaw: 0.002 }
    }
}

/// Marks a pawn as possessed and owns its input history for prediction + reconciliation.
///
/// - Client: added to the pawn the local player controls
/// - Server: not used (server applies inputs directly from network messages)
#[derive(Component)]
#[component(storage = "SparseSet")]
pub struct Possessed {
    pending_input: Option<PawnInputKind>,
}
impl Possessed {
    pub fn new(_capacity: usize) -> Self {
        Self { pending_input: None }
    }
    pub fn push(&mut self, input: PawnInputKind) {
        self.pending_input = Some(input);
    }
    pub fn consume(&mut self) -> Option<PawnInputKind> {
        self.pending_input.take()
    }
    /// peek at the most recently pushed input without consuming it.
    pub fn peek_newest(&self) -> Option<&PawnInputKind> {
        self.pending_input.as_ref()
    }
}

// SYSTEMS

/// System set covering all gather-input systems. Reconciliation runs before this.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GatherInputSet;

/// System set covering all `move_pawns` systems. Use for ordering against pawn movement.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovePawnsSet;

/// generic input consumption function for all pawn types
pub fn move_pawns<T: Pawn>()
-> impl Fn(ResMut<PhysicsWorld>, Query<(&mut Possessed, &RigidBodyHandleComponent, &mut T)>) {
    |mut world, mut pawns| {
        for (mut possessed, handle, mut component) in pawns.iter_mut() {
            let Some(input) = possessed.consume() else {
                continue;
            };
            component.apply_input(&mut world, handle, input);
        }
    }
}

/// Peeks the newest buffered input, stamps it with the current tick,
/// records it for replay, and sends it serialized over the unreliable channel.
/// Register in client/main.rs after GatherInputSet, before MovePawnsSet, gated on multiplayer.
#[cfg(feature = "client")]
pub fn send_pawn_input(
    quic: Option<ResMut<net::quic::QuicManager>>,
    pawns: Query<&Possessed>,
    mut predicted: ResMut<PredictedCommands>,
) {
    let Some(mut quic) = quic else { return };
    let Ok(possessed) = pawns.single() else {
        return;
    };
    let Some(input) = possessed.peek_newest().cloned() else {
        return;
    };
    let seq = predicted.record_input(input.clone());
    quic.send_to_server(net::quic::Channel::Unreliable, &MsgType::Input(seq, input));
}
