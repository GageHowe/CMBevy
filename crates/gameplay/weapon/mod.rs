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
    pawn::{CameraEffector, CameraShake, HeldWeaponMap, PlayerRegistry, WeaponSlots},
    projectile::FiredProjectile,
    sound::SoundQueue,
};

pub mod beamer;
pub mod coil_launcher;
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
        crate::register_spawnable(app, "pistol", pistol::spawn_pistol);
        crate::register_spawnable(app, "beamer", beamer::spawn_beamer);
        crate::register_spawnable(app, "rifle", rifle::spawn_rifle);
        crate::register_spawnable(app, "smg", smg::spawn_smg);
        crate::register_spawnable(app, "hail_mary", hail_mary::spawn_hail_mary);
        crate::register_spawnable(app, "thumper", thumper::spawn_thumper);
        crate::register_spawnable(app, "lobber", lobber::spawn_lobber);
        crate::register_spawnable(app, "coil_launcher", coil_launcher::spawn_coil_launcher);
        app.add_plugins((beamer::BeamerPlugin, weapon_flash::WeaponFlashPlugin))
            .configure_sets(
                FixedUpdate,
                SimulateItemSet.before(physics::physics_world::step_physics),
            )
            .add_systems(FixedUpdate, tick_weapon_state);
        app.add_systems(
            FixedUpdate,
            prepare_projectile_shots
                .before(tick_weapon_state)
                .in_set(SimulateItemSet),
        );
        #[cfg(feature = "client")]
        app.add_systems(
            FixedUpdate,
            update_weapon_zoom
                .before(prepare_projectile_shots)
                .in_set(SimulateItemSet),
        )
        .add_systems(
            FixedUpdate,
            (predict_projectile_shots, beamer::drive_beamers_client)
                .chain()
                .after(prepare_projectile_shots)
                .before(tick_weapon_state)
                .in_set(SimulateItemSet),
        );
        #[cfg(not(feature = "client"))]
        app.add_systems(
            FixedUpdate,
            drive_authoritative_projectiles
                .after(prepare_projectile_shots)
                .in_set(SimulateItemSet),
        );
    }
}

#[cfg(not(feature = "client"))]
fn drive_authoritative_projectiles(
    inputs: Query<(Entity, &ProjectileFire, &NetworkID)>,
    registry: Res<PlayerRegistry>,
    mut pawn_slots: Query<&mut WeaponSlots>,
    mut weapon_runtime: Query<(&mut WeaponState, &WeaponConfig)>,
    mut held_weapons: ResMut<HeldWeaponMap>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut net_ids: ResMut<net::message::NetworkIDResource>,
    mut quic: ResMut<QuicManager>,
) {
    for (weapon_entity, shot, weapon_net_id) in &inputs {
        let owner_conn = registry.conn_id_for_character(shot.input.shooter);
        fire_authoritative_with_replication(
            shot.input.shooter,
            weapon_entity,
            weapon_net_id,
            Some(shot.input.prediction_id),
            shot.input.origin,
            shot.dir,
            &mut pawn_slots,
            &mut weapon_runtime,
            &mut held_weapons,
            &mut commands,
            &mut world,
            &mut net_ids,
            Some(&mut quic),
            owner_conn,
            false,
        );
        commands.entity(weapon_entity).remove::<ProjectileFire>();
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimulateItemSet;

fn prepare_projectile_shots(
    mut weapons: Query<(
        Entity,
        &NetworkID,
        &WeaponFireInput,
        &mut WeaponState,
        &WeaponConfig,
    )>,
    mut commands: Commands,
) {
    for (entity, net_id, input, mut state, config) in &mut weapons {
        let Some(behavior) = config.projectile_behavior else {
            continue;
        };
        if input.reload_pressed {
            start_reload(&mut state, config);
        }
        let requested = input.want_fire && (!behavior.semi_auto || input.fire_pressed);
        if requested && consume_round(&mut state, config) {
            let dir = spread(
                input.aim_dir,
                behavior.spread,
                input.prediction_id as u64 ^ net_id.0,
            );
            commands
                .entity(entity)
                .insert(ProjectileFire { input: *input, dir });
        }
        commands.entity(entity).remove::<WeaponFireInput>();
    }
}

fn spread(aim: Vec3, radians: f32, seed: u64) -> Vec3 {
    let aim = aim.normalize_or_zero();
    if radians <= 0.0 || aim == Vec3::ZERO {
        return aim;
    }
    let mut rng = fastrand::Rng::with_seed(seed);
    let right = aim.any_orthonormal_vector();
    let up = aim.cross(right).normalize_or_zero();
    let yaw = rng.f32() * 2.0 * radians - radians;
    let pitch = rng.f32() * 2.0 * radians - radians;
    (aim + right * yaw.tan() + up * pitch.tan()).normalize_or_zero()
}

#[cfg(feature = "client")]
fn update_weapon_zoom(
    weapons: Query<(&WeaponFireInput, &WeaponConfig)>,
    mut camera: Query<&mut CameraEffector, With<Camera3d>>,
) {
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    for (input, config) in &weapons {
        camera.zoom_multiplier = if input.want_alt_fire {
            config.zoom_multiplier
        } else {
            1.0
        };
    }
}

#[cfg(feature = "client")]
fn predict_projectile_shots(
    shots: Query<(
        Entity,
        &ProjectileFire,
        &WeaponConfig,
        Option<&hail_mary::HailMaryComponent>,
    )>,
    net_ids: Query<&NetworkID>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut predicted: Option<ResMut<common::PredictedCommands>>,
    mut sound: Option<ResMut<SoundQueue>>,
    mut camera: Query<&mut CameraEffector, With<Camera3d>>,
) {
    for (weapon, shot, config, hail_mary) in &shots {
        let Some(projectile) = config.projectile else {
            continue;
        };
        let speed = config.prediction_projectile_speed.unwrap_or(0.0);
        let velocity = crate::projectile::projectile_velocity(
            &world,
            Some(shot.input.shooter),
            shot.dir,
            speed,
        );
        let shooter_velocity =
            crate::projectile::shooter_velocity(&world, Some(shot.input.shooter));
        let projectile_entity = crate::projectile::spawn(
            projectile,
            config.projectile_gravity_scale,
            shot.input.origin,
            velocity,
            shooter_velocity,
            &mut commands,
            &mut world,
            Some(shot.input.shooter),
            Some(shot.input.prediction_id),
        );
        if let Some(decorate) = config.decorate_projectile {
            commands.queue(move |world: &mut World| decorate(projectile_entity, world));
        }
        if let Some(flash) = hail_mary.and_then(|weapon| weapon.muzzle_flash) {
            commands.queue(move |world: &mut World| {
                weapon_flash::trigger_weapon_flash(world, flash);
            });
        }
        if let Some(behavior) = config.projectile_behavior {
            if let Some(sound) = sound.as_deref_mut() {
                sound.play_2d(behavior.sound);
            }
            if let Ok(mut camera) = camera.single_mut() {
                let zoom = if config.zoom_multiplier > 1.0 {
                    ((camera.current_zoom_factor() - 1.0) / (config.zoom_multiplier - 1.0))
                        .clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let scale = 1.0 + (behavior.zoomed_kick_scale - 1.0) * zoom;
                camera.add_kick(
                    (
                        behavior.kick_vertical.0 * scale,
                        behavior.kick_vertical.1 * scale,
                    ),
                    (
                        behavior.kick_horizontal.0 * scale,
                        behavior.kick_horizontal.1 * scale,
                    ),
                    behavior.kick_recovery,
                );
                if let Some(shake) = behavior.shake {
                    camera.add_shake(shake);
                }
            }
            if let Ok(shooter_net_id) = net_ids.get(shot.input.shooter) {
                let mass = if config.mass_scaled_shooter_impulse {
                    helpers::shooter_mass(&world, Some(shot.input.shooter))
                } else {
                    behavior.recoil_scale
                };
                world.apply_game_impulse(
                    shot.input.shooter,
                    -shot.dir * config.shooter_impulse * mass,
                    Some(shooter_net_id),
                    predicted.as_deref_mut(),
                );
            }
        }
        commands.entity(weapon).remove::<ProjectileFire>();
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
        &net::message::MsgType::WeaponState(weapon_net_id.clone(), weapon_state),
    );
}

pub fn send_entity_weapon_state(
    quic: &mut QuicManager,
    target: SendTarget,
    entity: bevy::ecs::world::EntityRef<'_>,
    weapon_net_id: &NetworkID,
) {
    let Some(weapon_state) = entity.get::<WeaponState>() else {
        return;
    };
    send_weapon_state(quic, target, weapon_net_id, *weapon_state);
}

pub fn broadcast_dirty_weapon_states(
    mut quic: ResMut<QuicManager>,
    weapon_q: Query<(&NetworkID, Ref<WeaponState>)>,
) {
    for (net_id, weapon_state) in &weapon_q {
        if !weapon_state.is_changed() {
            continue;
        }
        send_weapon_state(&mut quic, SendTarget::All, net_id, *weapon_state);
    }
}

pub fn apply_weapon_state_world(entity: Entity, weapon_state: WeaponState, world: &mut World) {
    if let Some(mut state) = world.get_mut::<WeaponState>(entity) {
        *state = weapon_state;
    } else if world.entities().contains(entity) {
        world.entity_mut(entity).insert(weapon_state);
    }
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

    /// why is this a bool? maybe we should change it to a percent with a max, or some kind of log function
    pub mass_scaled_shooter_impulse: bool,
    pub decorate_projectile: Option<fn(Entity, &mut World)>,
    pub projectile_behavior: Option<ProjectileBehavior>,
}

#[derive(Clone, Copy)]
pub struct ProjectileBehavior {
    pub semi_auto: bool,
    pub spread: f32,
    pub sound: &'static str,
    pub recoil_scale: f32,
    pub kick_vertical: (f32, f32),
    pub kick_horizontal: (f32, f32),
    pub kick_recovery: f32,
    pub zoomed_kick_scale: f32,
    pub shake: Option<CameraShake>,
}

#[derive(Component, Clone, Copy)]
#[component(storage = "SparseSet")]
struct ProjectileFire {
    input: WeaponFireInput,
    dir: Vec3,
}

#[derive(Component, Clone, Copy)]
#[component(storage = "SparseSet")]
/// Per-tick fire request forwarded from a possessed pawn to its active weapon.
pub struct WeaponFireInput {
    pub want_fire: bool,
    pub fire_pressed: bool,
    pub want_alt_fire: bool,
    pub alt_fire_pressed: bool,
    pub reload_pressed: bool,
    pub origin: Vec3,
    pub aim_dir: Vec3,
    pub shooter: Entity,
    pub tick: u64,
    pub prediction_id: u32,
}

/// Result of firing a held weapon, including both weapon-state mutation and projectile spawn data.
pub struct FiredHeldWeapon {
    pub weapon_net_id: NetworkID,
    pub weapon_state: WeaponState,
    pub fired: FiredProjectile,
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

pub fn fire_held_weapon(
    shooter_entity: Entity,
    weapon_entity: Entity,
    weapon_net_id: &NetworkID,
    temp_id: Option<u32>,
    origin: Vec3,
    dir: Vec3,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut net::message::NetworkIDResource,
    consume_ammo: bool,
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
    if consume_ammo && !consume_round(&mut weapon_state, weapon_config) {
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
    #[cfg(feature = "client")]
    if let Some(decorate_projectile) = weapon_config.decorate_projectile {
        commands.queue(move |world: &mut World| {
            decorate_projectile(fired.entity, world);
        });
    }
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

pub fn fire_authoritative_with_replication(
    shooter_entity: Entity,
    weapon_entity: Entity,
    weapon_net_id: &NetworkID,
    temp_id: Option<u32>,
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
    consume_ammo: bool,
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
        consume_ammo,
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
            if let Some(temp_id) = temp_id {
                quic.send(
                    SendTarget::One(conn_id),
                    Channel::Ordered,
                    &net::message::MsgType::ProjectileConfirm {
                        temp_id,
                        net_id: fired.fired.net_id,
                    },
                );
            }
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
    let Some(projectile_entity) = crate::find_entity_by_net_id(world, &projectile_net_id)
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
    if let Some(decorate_projectile) = config.decorate_projectile {
        decorate_projectile(projectile_entity, world);
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
