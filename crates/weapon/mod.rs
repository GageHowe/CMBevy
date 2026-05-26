use bevy::prelude::*;
pub use common::WeaponState;
#[cfg(feature = "client")]
use net::message::WeaponState as NetWeaponState;
use net::{
    message::{NetworkID, SpawnType},
    quic::{Channel, ConnectionId, QuicManager, SendTarget},
    replication::ReplicationAppExt,
};
use physics::physics_world::PhysicsWorld;

use crate::{
    pawn::{CameraEffector, HeldWeaponMap, PlayerRegistry, WeaponSlots},
    projectile::FiredProjectile,
    sound::SoundQueue,
};

pub mod beamer;
pub mod coil_launcher;
pub mod failsafe;
pub mod hail_mary;
pub mod helpers;
pub mod lobber;
pub mod pistol;
pub mod rifle;
pub mod smg;
pub mod thumper;
pub mod weapon_flash;

/// Shared weapon plugin.
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.replicate_component::<WeaponState>()
            .add_plugins((beamer::BeamerPlugin, weapon_flash::WeaponFlashPlugin))
            .add_systems(FixedUpdate, tick_weapon_state);
        #[cfg(feature = "client")]
        app.add_systems(
            FixedUpdate,
            (
                rifle::drive_rifles,
                pistol::drive_pistols,
                smg::drive_smgs,
                beamer::drive_beamers,
                hail_mary::drive_hail_marys,
                thumper::drive_thumpers,
                lobber::drive_lobbers,
                coil_launcher::drive_coil_launchers,
            )
                .before(tick_weapon_state),
        );
    }
}

pub fn send_weapon_state(
    quic: &mut QuicManager,
    target: SendTarget,
    weapon_net_id: &NetworkID,
    weapon_state: WeaponState,
) {
    quic.send(
        target,
        Channel::Ordered,
        &net::message::MsgType::ComponentUpdate(
            net::replication::component_update_for::<WeaponState, _>(
                weapon_net_id.clone(),
                &weapon_state,
            )
            .unwrap(),
        ),
    );
}

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

#[derive(Component, Clone)]
/// Static weapon tuning shared by client prediction and server authority.
pub struct WeaponConfig {
    pub display_name: &'static str,
    pub model_path: &'static str,
    pub collider_path: &'static str,
    pub crosshair_path: &'static str,
    pub prediction_projectile_speed: Option<f32>,
    pub zoom_multiplier: f32,
    /// Magazine capacity used by reload and depletion logic.
    pub magazine_size: u16,
    pub reserve_ammo: u16,
    /// Reload duration in fixed ticks.
    pub reload_ticks: u16,
    /// Time between shots in fixed ticks.
    pub fire_cooldown_ticks: u16,
    pub projectile: Option<crate::projectile::Projectile>,
    pub projectile_gravity_scale: f32,
    pub shooter_impulse: f32,
    pub mass_scaled_shooter_impulse: bool,
    pub decorate_projectile: fn(Entity, &mut World),
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

#[cfg(feature = "client")]
#[derive(Component, Clone, Copy)]
pub struct PendingWeaponInput(pub WeaponFireInput);

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

pub fn weapon_bundle<W: Component>(weapon: W, config: WeaponConfig) -> impl Bundle {
    let ammo_in_mag = config.magazine_size;
    let reserve_ammo = config.reserve_ammo;
    (
        weapon,
        config,
        WeaponState {
            ammo_in_mag,
            reserve_ammo,
            reload_ticks: 0,
            cooldown_ticks: 0,
        },
    )
}

#[cfg(feature = "client")]
pub fn drive_weapon_inputs<W: Component<Mutability = bevy::ecs::component::Mutable>>(
    weapons: &mut Query<(Entity, &mut W, &mut WeaponState, &WeaponConfig, &PendingWeaponInput)>,
    net_ids: Query<&NetworkID>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
    possessed: Query<Entity, With<crate::pawn::Possessed>>,
    mut camera_fx: Query<(&mut CameraEffector, &GlobalTransform), With<Camera3d>>,
    mut id_counter: Option<ResMut<crate::projectile::ProjectileIdCounter>>,
    mut predicted: Option<ResMut<common::PredictedCommands>>,
    update: fn(&mut W, &mut PhysicsWorld, &mut Commands, &mut FireCtx),
) {
    for (weapon_entity, mut weapon, mut weapon_state, weapon_config, pending) in weapons.iter_mut() {
        let input = pending.0;
        let local_shooter = possessed.single().ok() == Some(input.shooter);
        let mut local_camera = if local_shooter {
            camera_fx.single_mut().ok().map(|(cam_fx, _)| cam_fx)
        } else {
            None
        };
        let mut ctx = FireCtx {
            weapon: weapon_entity,
            want_fire: input.want_fire,
            want_alt_fire: input.want_alt_fire,
            alt_fire_pressed: input.alt_fire_pressed,
            reload_pressed: input.reload_pressed,
            origin: input.origin,
            aim_dir: input.aim_dir,
            shooter: Some(input.shooter),
            tick: input.tick,
            net_id: net_ids.get(weapon_entity).ok(),
            shooter_net_id: net_ids.get(input.shooter).ok(),
            sound: sound_queue.as_deref_mut(),
            camera: local_camera.as_deref_mut(),
            quic: quic.as_deref_mut(),
            id_counter: id_counter.as_mut().map(|c| &mut c.count),
            predicted: predicted.as_deref_mut(),
            weapon_state: &mut weapon_state,
            weapon_config: weapon_config.clone(),
        };
        update(&mut weapon, &mut world, &mut commands, &mut ctx);
        commands.entity(weapon_entity).remove::<PendingWeaponInput>();
    }
}

pub fn fire_held_weapon(
    shooter_entity: Entity,
    weapon_entity: Entity,
    weapon_net_id: &NetworkID,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut net::message::NetworkIDResource,
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
    if !consume_round(&mut weapon_state, weapon_config) {
        return None;
    }
    let Some(projectile) = weapon_config.projectile else {
        return None;
    };
    let weapon_state_after_fire = *weapon_state;
    let depleted = is_depleted(&weapon_state);
    let fired = crate::projectile::fire_authoritative(
        projectile,
        weapon_config.prediction_projectile_speed.unwrap_or(0.0),
        weapon_config.projectile_gravity_scale,
        weapon_config.shooter_impulse,
        weapon_config.mass_scaled_shooter_impulse,
        origin,
        dir,
        shooter_entity,
        temp_id,
        commands,
        world,
        net_ids,
    )?;
    #[cfg(feature = "client")]
    let decorate_projectile = weapon_config.decorate_projectile;
    #[cfg(feature = "client")]
    commands.queue(move |world: &mut World| {
        decorate_projectile(fired.entity, world);
    });
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
        Some(conn_id),
    ) {
        if let Ok((weapon_state, _)) = weapon_runtime.get_mut(weapon_entity) {
            send_weapon_state(quic, SendTarget::One(conn_id), &weapon_net_id, *weapon_state);
        }
    }
}

pub fn fire_authoritative_with_replication(
    shooter_entity: Entity,
    weapon_entity: Entity,
    weapon_net_id: &NetworkID,
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
    owner_conn: Option<ConnectionId>,
) -> bool {
    let Some(fired) = fire_held_weapon(
        shooter_entity,
        weapon_entity,
        weapon_net_id,
        temp_id,
        origin,
        dir,
        pawn_slots,
        weapon_runtime,
        held_weapons,
        commands,
        world,
        net_ids,
    ) else {
        return false;
    };

    let Some(quic) = quic else {
        return true;
    };
    match owner_conn {
        Some(conn_id) => {
            quic.send(
                SendTarget::AllExcept(conn_id),
                Channel::Unordered,
                &net::message::MsgType::ProjectileSpawn {
                    weapon: weapon_net_id.clone(),
                    net_id: fired.fired.net_id.clone(),
                    position: fired.fired.position,
                    starting_velocity: fired.fired.starting_velocity,
                    shooter_velocity: fired.fired.shooter_velocity,
                },
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
            &net::message::MsgType::ProjectileSpawn {
                weapon: weapon_net_id.clone(),
                net_id: fired.fired.net_id,
                position: fired.fired.position,
                starting_velocity: fired.fired.starting_velocity,
                shooter_velocity: fired.fired.shooter_velocity,
            },
        ),
    }
    true
}

pub fn spawn_remote_projectile(
    weapon_net_id: &NetworkID,
    projectile_net_id: NetworkID,
    position: Vec3,
    starting_velocity: Vec3,
    shooter_velocity: Vec3,
    world: &mut World,
) {
    let Some(weapon_entity) = crate::find_entity_by_net_id(world, weapon_net_id) else {
        return;
    };
    let Some(projectile_entity) =
        crate::find_entity_by_net_id(world, &projectile_net_id)
            .or_else(|| Some(world.spawn((projectile_net_id.clone(),)).id()))
    else {
        return;
    };
    let Some(config) = world.get::<WeaponConfig>(weapon_entity).cloned() else {
        return;
    };
    let Some(projectile) = config.projectile else {
        return;
    };
    crate::projectile::spawn_remote(
        projectile_entity,
        projectile,
        config.projectile_gravity_scale,
        position,
        starting_velocity,
        shooter_velocity,
        world,
    );
    #[cfg(feature = "client")]
    (config.decorate_projectile)(projectile_entity, world);
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
    if !started {
        send_weapon_state(quic, SendTarget::One(conn_id), &weapon_net_id, *weapon_state);
    }
}

pub fn handle_set_active_slot_request(
    conn_id: ConnectionId,
    active_primary: bool,
    registry: &PlayerRegistry,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    _quic: &mut QuicManager,
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
    let Some(_old_weapon_id) = old_active else {
        return;
    };
    let Ok((mut weapon_state, _)) = weapon_runtime.get_mut(old_weapon_entity) else {
        return;
    };
    cancel_reload(&mut weapon_state);
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

pub fn apply_zoom(ctx: &mut FireCtx) -> f32 {
    let Some(cam) = ctx.camera.as_mut() else {
        return 0.0;
    };
    let zoom_multiplier = if ctx.want_alt_fire {
        ctx.weapon_config.zoom_multiplier
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

pub fn is_weapon_kind(kind: &SpawnType) -> bool {
    matches!(
        kind,
        SpawnType::Pistol
            | SpawnType::Beamer
            | SpawnType::Rifle
            | SpawnType::Smg
            | SpawnType::Failsafe
            | SpawnType::HailMary
            | SpawnType::Thumper
            | SpawnType::Lobber
            | SpawnType::CoilLauncher
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
