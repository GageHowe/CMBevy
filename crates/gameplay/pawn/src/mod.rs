// pub mod kinds;
pub mod biped;
pub mod biped_ability;
mod camera_effects;
pub mod hovercraft;
pub mod mount;
pub mod spaceship;
pub mod vehicle;
pub mod weapon_slots;

// pub mod dep;

use std::collections::HashMap;

use bevy::prelude::*;
pub use biped::{BipedPawnComponent, PitchPivot, YawPivot};
pub use camera_effects::{CameraEffector, CameraShake};
#[cfg(feature = "client")]
use common::LocalControl;
pub use common::{BipedInput, PawnInput};
pub use hovercraft::HovercraftPawnComponent;
pub use mount::{CharacterMount, Mounted};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_rot};
pub use spaceship::SpaceshipPawnComponent;
pub use vehicle::VehicleComponent;
pub use weapon_slots::WeaponSlots;

use crate::net::{
    message::{BipedLook, Input, MsgType, NetworkID},
    quic::{Channel, ConnectionId, QuicManager, SendTarget},
};

#[derive(Resource, Default)]
pub struct PlayerRegistry {
    character_by_conn: HashMap<ConnectionId, (Entity, NetworkID)>,
}
impl PlayerRegistry {
    pub fn register_character(&mut self, conn_id: ConnectionId, entity: Entity, net_id: NetworkID) {
        self.character_by_conn.insert(conn_id, (entity, net_id));
    }

    pub fn character(&self, conn_id: ConnectionId) -> Option<(Entity, &NetworkID)> {
        self.character_by_conn
            .get(&conn_id)
            .map(|(entity, net_id)| (*entity, net_id))
    }

    pub fn remove_character_for_conn(
        &mut self,
        conn_id: ConnectionId,
    ) -> Option<(Entity, NetworkID)> {
        self.character_by_conn.remove(&conn_id)
    }

    pub fn remove_character(&mut self, entity: Entity) -> Option<(ConnectionId, NetworkID)> {
        let conn_id = self.conn_id_for_character(entity)?;
        let (_, net_id) = self.character_by_conn.remove(&conn_id)?;
        Some((conn_id, net_id))
    }

    pub fn conn_id_for_character(&self, entity: Entity) -> Option<ConnectionId> {
        self.character_by_conn
            .iter()
            .find_map(|(conn_id, (character, _))| (*character == entity).then_some(*conn_id))
    }

    pub fn controlled_count(&self) -> usize {
        self.character_by_conn.len()
    }

    pub fn controlled_conn_ids(&self) -> impl Iterator<Item = ConnectionId> + '_ {
        self.character_by_conn.keys().copied()
    }

    pub fn character_entries(
        &self,
    ) -> impl Iterator<Item = (&ConnectionId, &(Entity, NetworkID))> + '_ {
        self.character_by_conn.iter()
    }

    pub fn character_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.character_by_conn.values().map(|(entity, _)| *entity)
    }
}

/// Switches the input-controlled pawn for a connection and tells that client to possess it.
pub fn possess_pawn(conn_id: ConnectionId, net_id: &NetworkID, quic: &mut QuicManager) {
    send_possess(quic, conn_id, net_id);
}

pub fn send_possess(quic: &mut QuicManager, conn_id: ConnectionId, net_id: &NetworkID) {
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &crate::net::message::MsgType::Possess(crate::net::message::Possess(net_id.clone())),
    );
}

pub fn send_mount_state(
    quic: &mut QuicManager,
    target: SendTarget,
    biped_net_id: &NetworkID,
    parent_net_id: Option<&NetworkID>,
) {
    quic.send(
        target,
        Channel::Ordered,
        &crate::net::message::MsgType::MountState(crate::net::message::MountState {
            biped_net_id: biped_net_id.clone(),
            parent_net_id: parent_net_id.cloned(),
        }),
    );
}

/// Broadcasts all dirty replicated look state owned by bipeds (which have free look)
pub fn broadcast_dirty_look_updates(
    quic: &mut QuicManager,
    biped_looks: &mut Query<(&NetworkID, &mut biped::BipedPawnComponent)>,
) {
    for (net_id, mut biped) in biped_looks.iter_mut() {
        if !biped.look_sync_dirty {
            continue;
        }
        biped.look_sync_dirty = false;
        quic.send(
            SendTarget::All,
            Channel::Unreliable,
            &MsgType::BipedLook(BipedLook {
                net_id: net_id.clone(),
                yaw: biped.look_yaw,
                pitch: biped.look_pitch,
            }),
        );
    }
}

#[cfg(feature = "client")]
/// Applies a replicated look update to a remote pawn.
pub fn apply_remote_pawn_look(
    net_id: &NetworkID,
    yaw: f32,
    pitch: f32,
    local_net_id: Option<&NetworkID>,
    networked: &crate::NetworkEntityMap,
    bipeds: &mut Query<&mut biped::BipedPawnComponent>,
) {
    if local_net_id == Some(net_id) {
        return;
    }
    let Some(entity) = networked.get(net_id) else {
        return;
    };
    let Ok(mut biped) = bipeds.get_mut(entity) else {
        return;
    };
    biped.look_yaw = yaw;
    biped.look_pitch = pitch;
}

#[cfg(feature = "client")]
pub fn detach_camera(world: &mut World) {
    let mut camera_q = world.query_filtered::<Entity, With<Camera3d>>();
    let Some(camera) = camera_q.single(world).ok() else {
        return;
    };

    // If the camera's direct parent is a SpringArmPivot (intermediate entity), despawn it.
    // The pivot is a transient entity owned by the vehicle session; it must not outlive it.
    let pivot = world
        .get::<ChildOf>(camera)
        .map(|co| co.parent())
        .filter(|&p| world.get::<crate::spring_arm::SpringArmPivot>(p).is_some());

    let Ok(mut entity) = world.get_entity_mut(camera) else {
        return;
    };
    entity.remove_parent_in_place();

    if let Some(pivot_entity) = pivot {
        if let Ok(e) = world.get_entity_mut(pivot_entity) {
            e.despawn();
        }
    }
}

/// Pending respawns: conn_id -> (seconds_remaining, kind).
#[derive(Resource, Default)]
/// Respawn timers keyed by connection id.
pub struct PendingRespawns(pub HashMap<ConnectionId, (f32, String, crate::Team)>);

#[derive(Resource, Default)]
/// Reverse lookup from held weapon ids to the entity currently carrying them.
pub struct HeldWeaponMap(pub HashMap<NetworkID, Entity>);

/// Top-level plugin that registers all pawn submodules and shared pawn systems.
pub struct PawnPlugin;
impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        crate::register_spawnable(app, "biped", biped::spawn_biped);
        crate::register_spawnable(app, "spaceship", spaceship::spawn_spaceship);
        crate::register_spawnable(app, "hovercraft", hovercraft::spawn_hovercraft);
        #[cfg(feature = "client")]
        app.init_resource::<InteractionGate>()
            .init_resource::<InteractionHint>()
            .add_systems(
                PostUpdate,
                apply_camera_effects
                    .in_set(common::game_state::SimulationSystems)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
        app.add_plugins(biped_ability::BipedAbilityPlugin);
        app.add_plugins(biped::BipedPlugin);
        app.add_plugins(hovercraft::HovercraftPlugin);
        app.add_plugins(mount::MountPlugin);
        app.add_plugins(spaceship::SpaceshipPlugin);
        app.add_plugins(vehicle::VehiclePlugin);
    }
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Small debounce gate for client interaction input.
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
/// UI-facing interaction prompt text for the locally controlled player.
pub struct InteractionHint(pub Option<String>);

pub fn aim_dir(world: &PhysicsWorld, entity: Entity, input: Option<&PawnInput>) -> Option<Vec3> {
    let input = input?;
    world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| {
            rb_rot(rb)
                * Quat::from_rotation_y(input.look_yaw)
                * Quat::from_rotation_x(input.look_pitch)
                * Vec3::NEG_Z
        })
}

// CAMERA

/// runtime mouse sensitivity, set from the Settings resource by SettingsPlugin. Only needed by client.
#[derive(Resource)]
/// Runtime mouse/input sensitivity values applied by client control systems.
pub struct MouseSensitivity {
    pub base: f32,
    pub zoom_blend: f32,
    pub vehicle_pitch_yaw: f32,
    pub gamepad_look: f32,
    pub gamepad_move_deadzone: f32,
    pub gamepad_look_deadzone: f32,
    pub gamepad_invert_y: bool,
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

    let target_fov = ((fx.base_fov / 2.0).to_radians().tan() / fx.zoom_multiplier)
        .atan()
        .to_degrees()
        * 2.0;
    fx.current_fov += (target_fov - fx.current_fov) * (1.0 - (-FOV_LERP_SPEED * dt).exp());
    if let Projection::Perspective(ref mut p) = *proj {
        p.fov = fx.current_fov.to_radians();
    }
}

impl Default for MouseSensitivity {
    fn default() -> Self {
        Self {
            base: 0.002,
            zoom_blend: 1.0,
            vehicle_pitch_yaw: 0.002,
            gamepad_look: 3.0,
            gamepad_move_deadzone: 0.2,
            gamepad_look_deadzone: 0.15,
            gamepad_invert_y: false,
        }
    }
}

/// Marks a pawn as possessed and owns its input history for prediction + reconciliation.
///
/// - Client: added to the pawn the local player controls
/// - Server: not used (server applies inputs directly from network messages)
#[derive(Component)]
#[component(storage = "SparseSet")]
/// Marker for the single locally controlled pawn and its latest buffered input.
pub struct Controller {
    pub client: Option<ConnectionId>,
    input: Option<PawnInput>,
}
impl Controller {
    pub fn new(_capacity: usize) -> Self {
        Self {
            client: None,
            input: None,
        }
    }
    pub fn for_client(client: ConnectionId) -> Self {
        Self {
            client: Some(client),
            input: None,
        }
    }
    pub fn push(&mut self, input: PawnInput) {
        self.input = Some(input);
    }
    pub fn consume(&mut self) -> Option<PawnInput> {
        self.input.take()
    }
    /// peek at the most recently pushed input without consuming it.
    pub fn peek_newest(&self) -> Option<&PawnInput> {
        self.input.as_ref()
    }
    pub fn peek_newest_mut(&mut self) -> Option<&mut PawnInput> {
        self.input.as_mut()
    }
}
// SYSTEMS

/// System set covering all gather-input systems. Reconciliation runs before this.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GatherInputSet;

#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovePawnsSet;

#[cfg(feature = "client")]
fn apply_local_control(mut control: ResMut<LocalControl>, mut pawns: Query<&mut Controller>) {
    let Ok(mut pawn) = pawns.single_mut() else {
        return;
    };
    if let Some(input) = control.consume() {
        pawn.push(input);
    }
}

/// Peeks the newest buffered input, stamps it with the current tick,
/// records it for replay, and sends it serialized over the unreliable channel.
/// Register in client/main.rs after GatherInputSet, before MovePawnsSet, gated on multiplayer.
#[cfg(feature = "client")]
pub fn send_pawn_input(
    quic: Option<ResMut<crate::net::quic::QuicManager>>,
    mut control: ResMut<LocalControl>,
) {
    let Some(mut quic) = quic else { return };
    let Some((seq, input)) = control.take_newest_to_send() else {
        return;
    };
    quic.send_to_server(
        crate::net::quic::Channel::Unreliable,
        &MsgType::Input(Input {
            seq,
            input: input.clone(),
        }),
    );
    if input.item.primary_pressed
        || input.item.secondary_pressed
        || input.item.reload_pressed
        || input.ability1_pressed
        || input.melee_pressed
    {
        quic.send_to_server(
            crate::net::quic::Channel::Unordered, // shouldnt this be unreliable? it'll be way too late if resent
            &MsgType::Input(Input {
                seq,
                input: input.clone(),
            }),
        );
    }
}
