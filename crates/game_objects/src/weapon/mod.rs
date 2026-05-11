#[cfg(feature = "client")]
use bevy::ecs::system::{In, SystemId};
use bevy::prelude::*;
pub use common::WeaponState;
#[cfg(feature = "client")]
use net::message::WeaponState as NetWeaponState;
use net::{
    message::NetworkID,
    quic::{Channel, ConnectionId, QuicManager, SendTarget},
};
use physics::physics_world::PhysicsWorld;

use crate::{
    GameObjectKind,
    pawn::{CameraEffector, HeldWeaponMap, PlayerRegistry, WeaponSlots},
    projectile::FiredProjectile,
    sound::SoundQueue,
};

pub mod grenade_launcher;
pub mod hail_mary;
pub mod beamer;
pub mod helpers;
pub mod lobber;
pub mod coil_launcher;
pub mod pistol;
pub mod rifle;
pub mod thumper;

/// Type-erased authoritative projectile spawn function used by weapon configs.
pub type FireProjectileFn = fn(
    Vec3,
    Vec3,
    Entity,
    u64,
    Entity,
    u32,
    &mut Commands,
    &mut PhysicsWorld,
    &mut net::message::NetworkIDResource,
) -> Option<FiredProjectile>;

/// Shared weapon plugin.
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            rifle::RiflePlugin,
            pistol::PistolPlugin,
            beamer::BeamerPlugin,
            hail_mary::HailMaryPlugin,
            thumper::ThumperPlugin,
            lobber::LobberPlugin,
            coil_launcher::CoilLauncherPlugin,
            grenade_launcher::GrenadeLauncherPlugin,
        ))
        .add_systems(FixedUpdate, tick_weapon_state);
    }
}

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

#[derive(Component, Clone)]
/// Static weapon tuning shared by client prediction and server authority.
pub struct WeaponConfig {
    /// Magazine capacity used by reload and depletion logic.
    pub magazine_size: u16,
    /// Reload duration in fixed ticks.
    pub reload_ticks: u16,
    /// Time between shots in fixed ticks.
    pub fire_cooldown_ticks: u16,
    /// Projectile type this weapon is expected to spawn.
    pub projectile_kind: net::message::GameObjectKind,
    /// Type-erased authoritative projectile spawn hook.
    pub fire_projectile: FireProjectileFn,
}

/// Type-erased fire hook for the weapon entity. Biped code only forwards input here.
#[cfg(feature = "client")]
#[derive(Component, Clone, Copy)]
pub struct WeaponDriver {
    pub fixed_update: SystemId<In<WeaponFireInput>>,
}

#[cfg(feature = "client")]
#[derive(Clone, Copy)]
/// Per-tick fire request forwarded from a possessed pawn to its active weapon.
pub struct WeaponFireInput {
    pub weapon: Entity,
    pub want_fire: bool,
    pub want_alt_fire: bool,
    pub alt_fire_pressed: bool,
    pub reload_pressed: bool,
    pub origin: Vec3,
    pub aim_dir: Vec3,
    pub shooter: Entity,
    pub tick: u64,
}

/// Result of firing a held weapon, including both weapon-state mutation and projectile spawn data.
pub struct FiredHeldWeapon {
    pub weapon_net_id: NetworkID,
    pub weapon_state: WeaponState,
    pub fired: FiredProjectile,
}

/// All context a weapon's fixed_update may need: input buttons and output channels.
/// Fields are optional so weapons compile and behave correctly on the server (no sound/camera).
pub struct FireCtx<'a> {
    /// Weapon entity currently executing its fire/update logic.
    pub weapon: Entity,
    pub want_fire: bool,
    pub want_alt_fire: bool,
    pub alt_fire_pressed: bool,
    pub reload_pressed: bool,
    pub origin: Vec3,
    pub aim_dir: Vec3,
    pub shooter: Option<Entity>,
    pub tick: u64,
    /// NetworkID of the weapon entity — used to tell the server what fired.
    pub net_id: Option<&'a NetworkID>,
    /// NetworkID of the pawn carrying the weapon — included in fire packets so other clients can find the ghost.
    pub shooter_net_id: Option<&'a NetworkID>,
    /// Push to play a one-shot sound this frame.
    pub sound: Option<&'a mut SoundQueue>,
    /// Local player camera; None on server or before possession.
    pub camera: Option<&'a mut CameraEffector>,
    /// QUIC manager for sending Fire messages; None in singleplayer.
    pub quic: Option<&'a mut net::quic::QuicManager>,
    /// Projectile id counter for client-side prediction; None in singleplayer / on server.
    pub id_counter: Option<&'a mut u32>,
    /// Local predicted command history so weapons can replay non-input impulses during reconciliation.
    pub predicted: Option<&'a mut common::PredictedCommands>,
    /// Mutable authoritative/predicted state for this weapon instance.
    pub weapon_state: &'a mut WeaponState,
    /// Static weapon config copied in so fire code can use it without extra queries.
    pub weapon_config: WeaponConfig,
}

/// Per-weapon-type firing logic. Implement on each weapon component.
/// Weapons own their complete fire behavior: cooldowns, projectiles, sounds, camera kick, networking.
pub trait Weapon: Component<Mutability = bevy::ecs::component::Mutable> + Default {
    const MODEL_PATH: &'static str;
    const COLLIDER_PATH: &'static str;
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair013.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = None;
    const ZOOM_MULTIPLIER: f32 = 1.0;
    const MAGAZINE_SIZE: u16;
    const RESERVE_AMMO: u16;
    const RELOAD_TICKS: u16;
    const FIRE_COOLDOWN_TICKS: u16;
    const PROJECTILE_KIND: net::message::GameObjectKind;
    const FIRE_PROJECTILE: FireProjectileFn;
    /// Called every FixedPreUpdate tick when this weapon is the active slot.
    /// The weapon reads input from ctx, spawns projectiles/effects, and calls ctx helpers as needed.
    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    );
}

#[cfg(feature = "client")]
pub fn weapon_bundle<W: Weapon + 'static>(weapon: W, world: &mut World) -> impl Bundle {
    (
        weapon,
        WeaponConfig::new::<W>(),
        WeaponState {
            ammo_in_mag: W::MAGAZINE_SIZE,
            reserve_ammo: W::RESERVE_AMMO,
            reload_ticks: 0,
            cooldown_ticks: 0,
        },
        WeaponDriver {
            fixed_update: world.register_system_cached(fire_weapon::<W>),
        },
    )
}

#[cfg(not(feature = "client"))]
pub fn weapon_bundle<W: Weapon>(weapon: W, _world: &mut World) -> impl Bundle {
    (
        weapon,
        WeaponConfig::new::<W>(),
        WeaponState {
            ammo_in_mag: W::MAGAZINE_SIZE,
            reserve_ammo: W::RESERVE_AMMO,
            reload_ticks: 0,
            cooldown_ticks: 0,
        },
    )
}

#[cfg(feature = "client")]
pub fn fire_weapon<W: Weapon>(
    In(input): In<WeaponFireInput>,
    mut weapons: Query<(&mut W, &mut WeaponState, &WeaponConfig)>,
    net_ids: Query<&NetworkID>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
    possessed: Query<Entity, With<crate::pawn::Possessed>>,
    mut camera_fx: Query<(&mut CameraEffector, &GlobalTransform), With<Camera3d>>,
    mut id_counter: Option<ResMut<crate::projectile::ProjectileIdCounter>>,
    mut predicted: Option<ResMut<common::PredictedCommands>>,
) {
    let Ok((mut weapon, mut weapon_state, weapon_config)) = weapons.get_mut(input.weapon) else {
        return;
    };
    let local_shooter = possessed.single().ok() == Some(input.shooter);
    let mut local_camera = if local_shooter {
        camera_fx.single_mut().ok().map(|(cam_fx, _)| cam_fx)
    } else {
        None
    };
    let mut ctx = FireCtx {
        weapon: input.weapon,
        want_fire: input.want_fire,
        want_alt_fire: input.want_alt_fire,
        alt_fire_pressed: input.alt_fire_pressed,
        reload_pressed: input.reload_pressed,
        origin: input.origin,
        aim_dir: input.aim_dir,
        shooter: Some(input.shooter),
        tick: input.tick,
        net_id: net_ids.get(input.weapon).ok(),
        shooter_net_id: net_ids.get(input.shooter).ok(),
        sound: sound_queue.as_deref_mut(),
        camera: local_camera.as_deref_mut(),
        quic: quic.as_deref_mut(),
        id_counter: id_counter.as_mut().map(|c| &mut c.count),
        predicted: predicted.as_deref_mut(),
        weapon_state: &mut weapon_state,
        weapon_config: weapon_config.clone(),
    };
    weapon.fixed_update(&mut world, &mut commands, &mut ctx);
}

pub fn fire_held_weapon(
    shooter_entity: Entity,
    weapon_entity: Entity,
    weapon_net_id: &NetworkID,
    kind: Option<GameObjectKind>,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut net::message::NetworkIDResource,
    tick: u64,
) -> Option<FiredHeldWeapon> {
    let Ok(mut slots) = pawn_slots.get_mut(shooter_entity) else {
        return None;
    };
    if !slots.contains_net_id(weapon_net_id) {
        return None;
    }
    let Ok((mut weapon_state, weapon_config)) = weapon_runtime.get_mut(weapon_entity) else {
        return None;
    };
    if kind.is_some_and(|kind| weapon_config.projectile_kind != kind)
        || !consume_round(&mut weapon_state, weapon_config)
    {
        return None;
    }
    let weapon_state_after_fire = *weapon_state;
    let depleted = is_depleted(&weapon_state);
    let fired = (weapon_config.fire_projectile)(
        origin,
        dir,
        shooter_entity,
        tick,
        weapon_entity,
        temp_id,
        commands,
        world,
        net_ids,
    )?;
    drop(weapon_state);

    if depleted {
        held_weapons.0.remove(weapon_net_id);
        slots.remove_by_net_id(weapon_net_id);
        commands.entity(weapon_entity).despawn();
    }

    Some(FiredHeldWeapon {
        weapon_net_id: weapon_net_id.clone(),
        weapon_state: weapon_state_after_fire,
        fired,
    })
}

pub fn handle_fire_request(
    conn_id: ConnectionId,
    weapon_net_id: NetworkID,
    kind: GameObjectKind,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
    registry: &PlayerRegistry,
    all_networked: &crate::NetworkEntityMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut net::message::NetworkIDResource,
    quic: &mut QuicManager,
    tick: u64,
) {
    let Some((shooter_entity, _)) = registry.character(conn_id) else {
        return;
    };
    let shooter_holds = pawn_slots
        .get(shooter_entity)
        .map(|s| s.contains_net_id(&weapon_net_id))
        .unwrap_or(false);
    if !shooter_holds {
        return;
    }
    let Some(weapon_entity) = all_networked.get(&weapon_net_id) else {
        return;
    };
    if !fire_authoritative_with_replication(
        shooter_entity,
        weapon_entity,
        &weapon_net_id,
        kind,
        temp_id,
        origin,
        dir,
        pawn_slots,
        weapon_runtime,
        held_weapons,
        commands,
        world,
        net_ids,
        Some(quic),
        tick,
        Some(conn_id),
    ) {
        if let Ok((weapon_state, _)) = weapon_runtime.get_mut(weapon_entity) {
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &net::message::MsgType::WeaponState(weapon_net_id, *weapon_state),
            );
        }
    }
}

pub fn fire_authoritative_with_replication(
    shooter_entity: Entity,
    weapon_entity: Entity,
    weapon_net_id: &NetworkID,
    kind: GameObjectKind,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut net::message::NetworkIDResource,
    quic: Option<&mut QuicManager>,
    tick: u64,
    owner_conn: Option<ConnectionId>,
) -> bool {
    let Some(fired) = fire_held_weapon(
        shooter_entity,
        weapon_entity,
        weapon_net_id,
        Some(kind),
        temp_id,
        origin,
        dir,
        pawn_slots,
        weapon_runtime,
        held_weapons,
        commands,
        world,
        net_ids,
        tick,
    ) else {
        return false;
    };

    let Some(quic) = quic else {
        return true;
    };
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &net::message::MsgType::WeaponState(fired.weapon_net_id.clone(), fired.weapon_state),
    );
    match owner_conn {
        Some(conn_id) => {
            quic.send(
                SendTarget::AllExcept(conn_id),
                Channel::Unordered,
                &net::message::MsgType::SpawnCommand(fired.fired.spawn_cmd),
            );
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &net::message::MsgType::ProjectileConfirm {
                    temp_id,
                    net_id: fired.fired.net_id,
                },
            );
        }
        None => quic.send(
            SendTarget::All,
            Channel::Unordered,
            &net::message::MsgType::SpawnCommand(fired.fired.spawn_cmd),
        ),
    }
    true
}

pub fn handle_reload_request(
    conn_id: ConnectionId,
    weapon_net_id: NetworkID,
    registry: &PlayerRegistry,
    all_networked: &crate::NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    quic: &mut QuicManager,
) {
    let Some((shooter_entity, _)) = registry.character(conn_id) else {
        return;
    };
    let shooter_holds = pawn_slots
        .get(shooter_entity)
        .map(|s| s.contains_net_id(&weapon_net_id))
        .unwrap_or(false);
    if !shooter_holds {
        return;
    }
    let Some(weapon_entity) = all_networked.get(&weapon_net_id) else {
        return;
    };
    let Ok((mut weapon_state, weapon_config)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    let started = start_reload(&mut weapon_state, weapon_config);
    quic.send(
        if started {
            SendTarget::All
        } else {
            SendTarget::One(conn_id)
        },
        Channel::Ordered,
        &net::message::MsgType::WeaponState(weapon_net_id, *weapon_state),
    );
}

pub fn handle_set_active_slot_request(
    conn_id: ConnectionId,
    active_primary: bool,
    registry: &PlayerRegistry,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    quic: &mut QuicManager,
) {
    let Some((player_entity, _)) = registry.controlled_pawn(conn_id) else {
        return;
    };
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let old_active = slots.active().0.clone();
    let old_active_entity = slots.active().1;
    slots.set_active_primary(active_primary);
    let new_active = slots.active().0.clone();
    if old_active == new_active {
        return;
    }
    let Some(old_weapon_entity) = old_active_entity else {
        return;
    };
    let Some(old_weapon_id) = old_active else {
        return;
    };
    let Ok((mut weapon_state, _)) = weapon_runtime.get_mut(old_weapon_entity) else {
        return;
    };
    cancel_reload(&mut weapon_state);
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &net::message::MsgType::WeaponState(old_weapon_id, *weapon_state),
    );
}

pub fn handle_drop_request(
    conn_id: ConnectionId,
    registry: &PlayerRegistry,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    held_weapons: &mut HeldWeaponMap,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    commands: &mut Commands,
    quic: &mut QuicManager,
    drop_dir: Vec3,
) {
    let Some((player_entity, player_net_id)) = registry.character(conn_id) else {
        return;
    };
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    let Some((weapon_id, weapon_entity)) = slots.remove_active() else {
        return;
    };
    drop(slots);
    helpers::drop_from_owner(
        weapon_id,
        player_net_id.clone(),
        weapon_entity,
        player_entity,
        drop_dir,
        world,
        weapon_runtime,
        held_weapons,
        commands,
        quic,
    );
}

pub fn handle_interact_pickup_request(
    player_entity: Entity,
    player_net_id: NetworkID,
    target_entity: Entity,
    target_net_id: NetworkID,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    commands: &mut Commands,
    interactables: &Query<&crate::interaction::Interactable>,
    drop_dir: Vec3,
) {
    helpers::interact_pickup(
        player_entity,
        player_net_id,
        target_entity,
        target_net_id,
        quic,
        world,
        weapon_runtime,
        held_weapons,
        pawn_slots,
        commands,
        interactables,
        drop_dir,
    );
}

pub fn apply_zoom<W: Weapon>(ctx: &mut FireCtx) -> f32 {
    let Some(cam) = ctx.camera.as_mut() else {
        return 0.0;
    };
    let zoom_multiplier = if ctx.want_alt_fire {
        W::ZOOM_MULTIPLIER
    } else {
        1.0
    }
    .max(1.0);
    cam.zoom_multiplier = zoom_multiplier;
    if zoom_multiplier <= 1.0 {
        return 0.0;
    }
    ((cam.current_zoom_factor() - 1.0) / (zoom_multiplier - 1.0)).clamp(0.0, 1.0)
}

impl WeaponConfig {
    pub fn new<W: Weapon>() -> Self {
        Self {
            magazine_size: W::MAGAZINE_SIZE,
            reload_ticks: W::RELOAD_TICKS,
            fire_cooldown_ticks: W::FIRE_COOLDOWN_TICKS,
            projectile_kind: W::PROJECTILE_KIND,
            fire_projectile: W::FIRE_PROJECTILE,
        }
    }
}

pub fn tick_weapon_state(
    mut weapons: Query<(&mut WeaponState, &WeaponConfig), With<WeaponComponent>>,
) {
    for (mut state, config) in &mut weapons {
        state.cooldown_ticks = state.cooldown_ticks.saturating_sub(1);
        if state.reload_ticks == 0 {
            continue;
        }
        state.reload_ticks -= 1;
        if state.reload_ticks > 0 {
            continue;
        }
        let need = config.magazine_size.saturating_sub(state.ammo_in_mag);
        if need == 0 || state.reserve_ammo == 0 {
            continue;
        }
        let refill = need.min(state.reserve_ammo);
        state.ammo_in_mag += refill;
        state.reserve_ammo -= refill;
    }
}

pub fn start_reload(state: &mut WeaponState, config: &WeaponConfig) -> bool {
    if state.reload_ticks > 0
        || state.ammo_in_mag >= config.magazine_size
        || state.reserve_ammo == 0
    {
        return false;
    }
    state.reload_ticks = config.reload_ticks;
    true
}

pub fn cancel_reload(state: &mut WeaponState) {
    state.reload_ticks = 0;
}

pub fn can_fire(state: &WeaponState) -> bool {
    state.reload_ticks == 0 && state.cooldown_ticks == 0 && state.ammo_in_mag > 0
}

pub fn is_depleted(state: &WeaponState) -> bool {
    state.ammo_in_mag == 0 && state.reserve_ammo == 0
}

pub fn consume_round(state: &mut WeaponState, config: &WeaponConfig) -> bool {
    if !can_fire(state) {
        if state.reload_ticks == 0 && state.cooldown_ticks == 0 && state.ammo_in_mag == 0 {
            start_reload(state, config);
        }
        return false;
    }
    state.ammo_in_mag -= 1;
    state.cooldown_ticks = config.fire_cooldown_ticks;
    // Empty mags should immediately enter reload so client prediction and server authority
    // stay on the same state path after the last shot.
    if state.ammo_in_mag == 0 {
        start_reload(state, config);
    }
    true
}

pub fn is_weapon_kind(kind: &common::GameObjectKind) -> bool {
    matches!(
        kind,
        common::GameObjectKind::Pistol
            | common::GameObjectKind::Beamer
            | common::GameObjectKind::Rifle
            | common::GameObjectKind::HailMary
            | common::GameObjectKind::Thumper
            | common::GameObjectKind::Lobber
            | common::GameObjectKind::CoilLauncher
            | common::GameObjectKind::GrenadeLauncher
    )
}

#[cfg(feature = "client")]
pub fn apply_weapon_state(
    net_id: &NetworkID,
    state: NetWeaponState,
    networked: &crate::NetworkEntityMap,
    weapon_states: &mut Query<&mut WeaponState>,
) {
    let Some(entity) = networked.get(net_id) else {
        return;
    };
    let Ok(mut weapon_state) = weapon_states.get_mut(entity) else {
        return;
    };
    *weapon_state = state;
}
