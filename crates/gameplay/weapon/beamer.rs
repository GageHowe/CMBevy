#[cfg(feature = "client")]
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;
use net::quic::{Channel, ConnectionId, QuicManager, SendTarget};
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::*;
#[cfg(feature = "client")]
use crate::flash::{FlashMaterial, FlashMaterialUniform, update_flash_material};
use crate::{
    NetworkEntityMap,
    health::{DamageCause, Health, LastDamageSource, attribute_damage},
    pawn::{PlayerRegistry, WeaponSlots},
};

pub const MAGAZINE_SIZE: u16 = 80;
pub const RESERVE_AMMO: u16 = 160;
pub const RELOAD_TICKS: u16 = 120;
pub const COOLDOWN_TICKS: u16 = 24;
pub const CHARGE_TICKS: u16 = 45;
pub const DAMAGE_INTERVAL_TICKS: u16 = 4;
pub const DAMAGE_PER_TICK: f32 = 8.0;
pub const RANGE: f32 = 180.0;

#[derive(Clone, Copy, PartialEq, Eq, Reflect, Default)]
pub enum BeamPhase {
    #[default]
    Idle,
    Charging,
    Beaming,
}

#[derive(Component, Reflect)]
pub struct BeamerComponent {
    pub phase: BeamPhase,
    pub phase_started_tick: u64,
    pub damage_tick_accum: u16,
    pub beam_origin: Vec3,
    pub beam_dir: Vec3,
    pub last_server_damage_tick: u64,
}

impl Default for BeamerComponent {
    fn default() -> Self {
        Self {
            phase: BeamPhase::Idle,
            phase_started_tick: 0,
            damage_tick_accum: 0,
            beam_origin: Vec3::ZERO,
            beam_dir: Vec3::ZERO,
            last_server_damage_tick: 0,
        }
    }
}

pub struct BeamerPlugin;
impl Plugin for BeamerPlugin {
    fn build(&self, _app: &mut App) {
        #[cfg(feature = "client")]
        _app.add_systems(Update, (add_visuals, sync_visuals));
    }
}

pub const CONFIG: WeaponConfig = WeaponConfig {
    display_name: "Beamer",
    model_path: "models/placeholder_beamer.glb#Scene0",
    collider_path: "collision/placeholder_ar.obj",
    crosshair_path: "textures/crosshairs/crosshair007.png",
    prediction_projectile_speed: None,
    zoom_multiplier: 1.8,
    magazine_size: MAGAZINE_SIZE,
    reserve_ammo: RESERVE_AMMO,
    reload_ticks: RELOAD_TICKS,
    fire_cooldown_ticks: COOLDOWN_TICKS,
    projectile: None,
    projectile_gravity_scale: 0.0,
    shooter_impulse: 0.0,
    mass_scaled_shooter_impulse: false,
    decorate_projectile,
};

#[cfg(feature = "client")]
fn update_beamer(
    weapon: &mut BeamerComponent,
    _world: &mut PhysicsWorld,
    _commands: &mut Commands,
    ctx: &mut FireCtx,
) {
    apply_zoom(ctx);

    if ctx.reload_pressed {
        end_local_beam(weapon, ctx, true);
        super::start_reload(ctx.weapon_state, &ctx.weapon_config);
        return;
    }

    if !ctx.want_fire {
        end_local_beam(weapon, ctx, true);
        return;
    }

    if weapon.phase == BeamPhase::Idle {
        if !super::can_fire(ctx.weapon_state) {
            return;
        }
        weapon.phase = BeamPhase::Charging;
        weapon.phase_started_tick = ctx.tick;
        weapon.damage_tick_accum = 0;
        weapon.beam_origin = ctx.origin;
        weapon.beam_dir = ctx.aim_dir.normalize_or_zero();
        #[cfg(feature = "client")]
        if let (Some(quic), Some(weapon_net_id)) = (ctx.quic.as_deref_mut(), ctx.net_id) {
            quic.send_to_server(
                Channel::Ordered,
                &net::message::MsgType::StartBeamCharge(weapon_net_id.clone()),
            );
        }
        return;
    }

    if weapon.phase == BeamPhase::Charging {
        weapon.beam_origin = ctx.origin;
        weapon.beam_dir = ctx.aim_dir.normalize_or_zero();
        if ctx.tick.saturating_sub(weapon.phase_started_tick) + 1 < CHARGE_TICKS as u64 {
            return;
        }
        weapon.phase = BeamPhase::Beaming;
        weapon.phase_started_tick = ctx.tick;
        weapon.damage_tick_accum = DAMAGE_INTERVAL_TICKS - 1;
        #[cfg(feature = "client")]
        if let (Some(quic), Some(weapon_net_id)) = (ctx.quic.as_deref_mut(), ctx.net_id) {
            quic.send_to_server(
                Channel::Ordered,
                &net::message::MsgType::StartBeam {
                    weapon: weapon_net_id.clone(),
                    origin: ctx.origin,
                    dir: weapon.beam_dir,
                },
            );
        }
    }

    if weapon.phase != BeamPhase::Beaming {
        return;
    }

    weapon.beam_origin = ctx.origin;
    weapon.beam_dir = ctx.aim_dir.normalize_or_zero();
    if weapon.beam_dir == Vec3::ZERO {
        return;
    }

    weapon.damage_tick_accum += 1;
    if weapon.damage_tick_accum < DAMAGE_INTERVAL_TICKS {
        return;
    }
    weapon.damage_tick_accum = 0;

    if ctx.weapon_state.ammo_in_mag == 0 {
        end_local_beam(weapon, ctx, true);
        super::start_reload(ctx.weapon_state, &ctx.weapon_config);
        return;
    }
    ctx.weapon_state.ammo_in_mag -= 1;
    if ctx.weapon_state.ammo_in_mag == 0 {
        super::start_reload(ctx.weapon_state, &ctx.weapon_config);
    }

    #[cfg(feature = "client")]
    let hit = beam_hit(_world, ctx.origin, weapon.beam_dir, ctx.shooter);
    #[cfg(feature = "client")]
    if let Some(weapon_net_id) = ctx.net_id.cloned() {
        queue_beam_hit_report(_commands, weapon_net_id, ctx.origin, weapon.beam_dir, hit);
    } else if ctx.quic.is_none() {
        let shooter = ctx.shooter;
        _commands.queue(move |world: &mut World| {
            apply_singleplayer_beam_hit(world, shooter, hit.map(|(entity, _)| entity));
        });
    }

    if ctx.weapon_state.ammo_in_mag == 0 {
        end_local_beam(weapon, ctx, false);
    }
}

#[cfg(feature = "client")]
pub fn drive_beamers(
    mut weapons: Query<(
        Entity,
        &mut BeamerComponent,
        &mut WeaponState,
        &WeaponConfig,
        &PendingWeaponInput,
    )>,
    net_ids: Query<&net::message::NetworkID>,
    world: ResMut<PhysicsWorld>,
    commands: Commands,
    quic: Option<ResMut<QuicManager>>,
    sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
    possessed: Query<Entity, With<crate::pawn::Possessed>>,
    camera_fx: Query<(&mut crate::pawn::CameraEffector, &GlobalTransform), With<Camera3d>>,
    id_counter: Option<ResMut<crate::projectile::ProjectileIdCounter>>,
    predicted: Option<ResMut<common::PredictedCommands>>,
) {
    super::drive_weapon_inputs(
        &mut weapons,
        net_ids,
        world,
        commands,
        quic,
        sound_queue,
        possessed,
        camera_fx,
        id_counter,
        predicted,
        update_beamer,
    );
}

pub fn spawn_beamer(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
    let weapon = weapon_bundle(BeamerComponent::default(), CONFIG);
    helpers::insert_generic_weapon(
        entity,
        cmd,
        "beamer",
        world,
        CONFIG.display_name,
        CONFIG.model_path,
        CONFIG.crosshair_path,
        CONFIG.prediction_projectile_speed,
        weapon,
    );
    helpers::make_generic_weapon_physics(
        entity,
        cmd,
        CONFIG.collider_path,
        ColliderBuilder::cuboid(0.2, 0.05, 0.4),
        world,
    );
    crate::insert_spawn_metadata(entity, world, Some(10.0), true, None, true);
}

fn decorate_projectile(_entity: Entity, _world: &mut World) {}

fn beam_hit(
    world: &PhysicsWorld,
    origin: Vec3,
    dir: Vec3,
    shooter: Option<Entity>,
) -> Option<(Entity, Vec3)> {
    let exclude = shooter.into_iter().collect::<Vec<_>>();
    let (entity, toi) = world.cast_ray(origin, dir, RANGE, &exclude)?;
    Some((entity, origin + dir * toi))
}

#[cfg(feature = "client")]
fn end_local_beam(beam: &mut BeamerComponent, ctx: &mut FireCtx, apply_cooldown: bool) {
    if beam.phase == BeamPhase::Idle {
        return;
    }
    beam.phase = BeamPhase::Idle;
    beam.damage_tick_accum = 0;
    beam.beam_dir = Vec3::ZERO;
    if apply_cooldown {
        ctx.weapon_state.cooldown_ticks = COOLDOWN_TICKS;
    }
    #[cfg(feature = "client")]
    if let (Some(quic), Some(weapon_net_id)) = (ctx.quic.as_deref_mut(), ctx.net_id) {
        quic.send_to_server(
            Channel::Ordered,
            &net::message::MsgType::EndBeam(weapon_net_id.clone()),
        );
    }
}

#[cfg(feature = "client")]
fn queue_beam_hit_report(
    commands: &mut Commands,
    weapon_net_id: net::message::NetworkID,
    origin: Vec3,
    dir: Vec3,
    hit: Option<(Entity, Vec3)>,
) {
    commands.queue(move |world: &mut World| {
        let target = hit.and_then(|(entity, _)| {
            world
                .get_resource::<NetworkEntityMap>()
                .and_then(|networked| networked.get_net_id_for_entity(entity).cloned())
        });
        let Some(mut quic) = world.get_resource_mut::<QuicManager>() else {
            return;
        };
        quic.send_to_server(
            Channel::Ordered,
            &net::message::MsgType::BeamHitReport {
                weapon: weapon_net_id.clone(),
                origin,
                dir,
                target,
            },
        );
    });
}

fn apply_singleplayer_beam_hit(world: &mut World, shooter: Option<Entity>, hit: Option<Entity>) {
    let Some(hit) = hit else {
        return;
    };
    if let Some(mut last_damage) = world.get_mut::<LastDamageSource>(hit) {
        last_damage.attacker = shooter;
        last_damage.cause = DamageCause::Projectile;
        last_damage.age_ticks = 0;
    }
    if let Some(mut health) = world.get_mut::<Health>(hit) {
        health.apply_damage(DAMAGE_PER_TICK);
    }
}

fn owned_beamer_entity(
    conn_id: ConnectionId,
    weapon_net_id: &net::message::NetworkID,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
) -> Option<(Entity, Entity)> {
    let (shooter_entity, _) = registry.character(conn_id)?;
    let shooter_holds = pawn_slots
        .get(shooter_entity)
        .map(|s| s.contains_net_id(weapon_net_id))
        .unwrap_or(false);
    if !shooter_holds {
        return None;
    }
    Some((shooter_entity, all_networked.get(weapon_net_id)?))
}

pub fn handle_start_charge_request(
    conn_id: ConnectionId,
    weapon_net_id: net::message::NetworkID,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    beamers: &mut Query<&mut BeamerComponent>,
    weapon_runtime: &mut Query<(&mut WeaponState, &super::WeaponConfig)>,
    quic: &mut QuicManager,
    tick: u64,
) {
    let Some((_shooter_entity, weapon_entity)) =
        owned_beamer_entity(conn_id, &weapon_net_id, registry, all_networked, pawn_slots)
    else {
        return;
    };
    let Ok((weapon_state, _)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    if !super::can_fire(&weapon_state) {
        return;
    }
    let Ok(mut beamer) = beamers.get_mut(weapon_entity) else {
        return;
    };
    if beamer.phase != BeamPhase::Idle {
        return;
    }
    beamer.phase = BeamPhase::Charging;
    beamer.phase_started_tick = tick;
    beamer.damage_tick_accum = 0;
    quic.send(
        SendTarget::AllExcept(conn_id),
        Channel::Ordered,
        &net::message::MsgType::StartBeamCharge(weapon_net_id),
    );
}

pub fn handle_start_beam_request(
    conn_id: ConnectionId,
    weapon_net_id: net::message::NetworkID,
    origin: Vec3,
    dir: Vec3,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    beamers: &mut Query<&mut BeamerComponent>,
    quic: &mut QuicManager,
    tick: u64,
) {
    let Some((_shooter_entity, weapon_entity)) =
        owned_beamer_entity(conn_id, &weapon_net_id, registry, all_networked, pawn_slots)
    else {
        return;
    };
    let Ok(mut beamer) = beamers.get_mut(weapon_entity) else {
        return;
    };
    if beamer.phase != BeamPhase::Charging
        || tick.saturating_sub(beamer.phase_started_tick) + 1 < CHARGE_TICKS as u64
    {
        return;
    }
    beamer.phase = BeamPhase::Beaming;
    beamer.phase_started_tick = tick;
    beamer.damage_tick_accum = 0;
    beamer.beam_origin = origin;
    beamer.beam_dir = dir.normalize_or_zero();
    quic.send(
        SendTarget::AllExcept(conn_id),
        Channel::Ordered,
        &net::message::MsgType::StartBeam {
            weapon: weapon_net_id,
            origin,
            dir,
        },
    );
}

pub fn handle_beam_hit_report(
    conn_id: ConnectionId,
    weapon_net_id: net::message::NetworkID,
    origin: Vec3,
    dir: Vec3,
    target: Option<net::message::NetworkID>,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    beamers: &mut Query<&mut BeamerComponent>,
    weapon_runtime: &mut Query<(&mut WeaponState, &super::WeaponConfig)>,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    quic: &mut QuicManager,
    tick: u64,
) {
    let Some((shooter_entity, weapon_entity)) =
        owned_beamer_entity(conn_id, &weapon_net_id, registry, all_networked, pawn_slots)
    else {
        return;
    };
    let Ok(mut beamer) = beamers.get_mut(weapon_entity) else {
        return;
    };
    if beamer.phase != BeamPhase::Beaming
        || tick.saturating_sub(beamer.last_server_damage_tick) < DAMAGE_INTERVAL_TICKS as u64
    {
        return;
    }
    let Ok((mut weapon_state, weapon_config)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    if weapon_state.ammo_in_mag == 0 {
        beamer.phase = BeamPhase::Idle;
        super::start_reload(&mut weapon_state, weapon_config);
        quic.send(
            SendTarget::AllExcept(conn_id),
            Channel::Ordered,
            &net::message::MsgType::EndBeam(weapon_net_id),
        );
        return;
    }
    weapon_state.ammo_in_mag -= 1;
    if weapon_state.ammo_in_mag == 0 {
        super::start_reload(&mut weapon_state, weapon_config);
        beamer.phase = BeamPhase::Idle;
    }
    beamer.last_server_damage_tick = tick;
    beamer.beam_origin = origin;
    beamer.beam_dir = dir.normalize_or_zero();

    if let Some(target_net_id) = target
        && let Some(target_entity) = all_networked.get(&target_net_id)
        && target_entity != shooter_entity
        && origin.distance_squared(beamer.beam_origin) < 9.0
        && origin.distance_squared(Vec3::ZERO).is_finite()
    {
        if let Ok(mut health) = health_q.get_mut(target_entity) {
            attribute_damage(
                last_damage_q,
                target_entity,
                Some(shooter_entity),
                DamageCause::Projectile,
            );
            health.apply_damage(DAMAGE_PER_TICK);
        }
    }

    quic.send(
        SendTarget::AllExcept(conn_id),
        Channel::Unordered,
        &net::message::MsgType::BeamHitReport {
            weapon: weapon_net_id.clone(),
            origin,
            dir,
            target: None,
        },
    );
    if beamer.phase == BeamPhase::Idle {
        quic.send(
            SendTarget::AllExcept(conn_id),
            Channel::Ordered,
            &net::message::MsgType::EndBeam(weapon_net_id),
        );
    }
}

pub fn handle_end_beam_request(
    conn_id: ConnectionId,
    weapon_net_id: net::message::NetworkID,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    beamers: &mut Query<&mut BeamerComponent>,
    weapon_runtime: &mut Query<(&mut WeaponState, &super::WeaponConfig)>,
    quic: &mut QuicManager,
) {
    let Some((_shooter_entity, weapon_entity)) =
        owned_beamer_entity(conn_id, &weapon_net_id, registry, all_networked, pawn_slots)
    else {
        return;
    };
    let Ok(mut beamer) = beamers.get_mut(weapon_entity) else {
        return;
    };
    if beamer.phase == BeamPhase::Idle {
        return;
    }
    beamer.phase = BeamPhase::Idle;
    beamer.damage_tick_accum = 0;
    beamer.beam_dir = Vec3::ZERO;
    if let Ok((mut weapon_state, _)) = weapon_runtime.get_mut(weapon_entity) {
        weapon_state.cooldown_ticks = COOLDOWN_TICKS;
    }
    quic.send(
        SendTarget::AllExcept(conn_id),
        Channel::Ordered,
        &net::message::MsgType::EndBeam(weapon_net_id),
    );
}

pub fn interrupt_server_beam(
    weapon_net_id: &net::message::NetworkID,
    weapon_entity: Entity,
    beamers: &mut Query<&mut BeamerComponent>,
    weapon_runtime: &mut Query<(&mut WeaponState, &super::WeaponConfig)>,
    quic: &mut QuicManager,
) {
    let Ok(mut beamer) = beamers.get_mut(weapon_entity) else {
        return;
    };
    if beamer.phase == BeamPhase::Idle {
        return;
    }
    beamer.phase = BeamPhase::Idle;
    beamer.damage_tick_accum = 0;
    beamer.beam_dir = Vec3::ZERO;
    if let Ok((mut weapon_state, _)) = weapon_runtime.get_mut(weapon_entity) {
        weapon_state.cooldown_ticks = COOLDOWN_TICKS;
    }
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &net::message::MsgType::EndBeam(weapon_net_id.clone()),
    );
}

pub fn tick_singleplayer_beam(
    weapon_entity: Entity,
    shooter: Entity,
    origin: Vec3,
    dir: Vec3,
    tick: u64,
    beamers: &mut Query<&mut BeamerComponent>,
    weapon_runtime: &mut Query<(&mut WeaponState, &super::WeaponConfig)>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Ok((mut weapon_state, weapon_config)) = weapon_runtime.get_mut(weapon_entity) else {
        return;
    };
    let Ok(mut beamer) = beamers.get_mut(weapon_entity) else {
        return;
    };
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return;
    }

    if beamer.phase == BeamPhase::Idle {
        if !super::can_fire(&weapon_state) {
            return;
        }
        beamer.phase = BeamPhase::Charging;
        beamer.phase_started_tick = tick;
        beamer.damage_tick_accum = 0;
        beamer.beam_origin = origin;
        beamer.beam_dir = dir;
        return;
    }

    if beamer.phase == BeamPhase::Charging {
        beamer.beam_origin = origin;
        beamer.beam_dir = dir;
        if tick.saturating_sub(beamer.phase_started_tick) + 1 < CHARGE_TICKS as u64 {
            return;
        }
        beamer.phase = BeamPhase::Beaming;
        beamer.phase_started_tick = tick;
        beamer.damage_tick_accum = DAMAGE_INTERVAL_TICKS - 1;
    }

    if beamer.phase != BeamPhase::Beaming {
        return;
    }

    beamer.beam_origin = origin;
    beamer.beam_dir = dir;
    beamer.damage_tick_accum += 1;
    if beamer.damage_tick_accum < DAMAGE_INTERVAL_TICKS {
        return;
    }
    beamer.damage_tick_accum = 0;

    if weapon_state.ammo_in_mag == 0 {
        beamer.phase = BeamPhase::Idle;
        beamer.beam_dir = Vec3::ZERO;
        weapon_state.cooldown_ticks = COOLDOWN_TICKS;
        super::start_reload(&mut weapon_state, weapon_config);
        return;
    }
    weapon_state.ammo_in_mag -= 1;
    if weapon_state.ammo_in_mag == 0 {
        super::start_reload(&mut weapon_state, weapon_config);
    }
    let hit = beam_hit(world, origin, dir, Some(shooter));
    commands.queue(move |world: &mut World| {
        apply_singleplayer_beam_hit(world, Some(shooter), hit.map(|(entity, _)| entity));
    });
    if weapon_state.ammo_in_mag == 0 {
        beamer.phase = BeamPhase::Idle;
        beamer.beam_dir = Vec3::ZERO;
    }
}

pub fn end_singleplayer_beam(
    weapon_entity: Entity,
    beamers: &mut Query<&mut BeamerComponent>,
    weapon_runtime: &mut Query<(&mut WeaponState, &super::WeaponConfig)>,
) {
    let Ok(mut beamer) = beamers.get_mut(weapon_entity) else {
        return;
    };
    if beamer.phase == BeamPhase::Idle {
        return;
    }
    beamer.phase = BeamPhase::Idle;
    beamer.damage_tick_accum = 0;
    beamer.beam_dir = Vec3::ZERO;
    if let Ok((mut weapon_state, _)) = weapon_runtime.get_mut(weapon_entity) {
        weapon_state.cooldown_ticks = COOLDOWN_TICKS;
    }
}

#[cfg(feature = "client")]
pub fn apply_remote_start_charge(world: &mut World, weapon_net_id: net::message::NetworkID) {
    let Some(weapon_entity) = world.resource::<NetworkEntityMap>().get(&weapon_net_id) else {
        return;
    };
    let Some(mut beamer) = world.get_mut::<BeamerComponent>(weapon_entity) else {
        return;
    };
    beamer.phase = BeamPhase::Charging;
    beamer.damage_tick_accum = 0;
}

#[cfg(feature = "client")]
pub fn apply_remote_start_beam(
    world: &mut World,
    weapon_net_id: net::message::NetworkID,
    origin: Vec3,
    dir: Vec3,
) {
    let Some(weapon_entity) = world.resource::<NetworkEntityMap>().get(&weapon_net_id) else {
        return;
    };
    let Some(mut beamer) = world.get_mut::<BeamerComponent>(weapon_entity) else {
        return;
    };
    beamer.phase = BeamPhase::Beaming;
    beamer.beam_origin = origin;
    beamer.beam_dir = dir.normalize_or_zero();
}

#[cfg(feature = "client")]
pub fn apply_remote_beam_report(
    world: &mut World,
    weapon_net_id: net::message::NetworkID,
    origin: Vec3,
    dir: Vec3,
) {
    let Some(weapon_entity) = world.resource::<NetworkEntityMap>().get(&weapon_net_id) else {
        return;
    };
    let Some(mut beamer) = world.get_mut::<BeamerComponent>(weapon_entity) else {
        return;
    };
    beamer.phase = BeamPhase::Beaming;
    beamer.beam_origin = origin;
    beamer.beam_dir = dir.normalize_or_zero();
}

#[cfg(feature = "client")]
pub fn apply_remote_end_beam(world: &mut World, weapon_net_id: net::message::NetworkID) {
    let Some(weapon_entity) = world.resource::<NetworkEntityMap>().get(&weapon_net_id) else {
        return;
    };
    let Some(mut beamer) = world.get_mut::<BeamerComponent>(weapon_entity) else {
        return;
    };
    beamer.phase = BeamPhase::Idle;
    beamer.damage_tick_accum = 0;
    beamer.beam_dir = Vec3::ZERO;
}

#[cfg(feature = "client")]
#[derive(Component)]
struct BeamerChargeVisual;

#[cfg(feature = "client")]
#[derive(Component)]
struct BeamerBeamVisual;

#[cfg(feature = "client")]
#[derive(Component)]
struct BeamerVisualRefs {
    charge: Entity,
    beam: Entity,
}

#[cfg(feature = "client")]
fn add_visuals(
    q: Query<Entity, (Added<BeamerComponent>, Without<BeamerVisualRefs>)>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flash_materials: ResMut<Assets<FlashMaterial>>,
) {
    for entity in &q {
        let charge = commands
            .spawn((
                BeamerChargeVisual,
                Mesh3d(meshes.add(bevy::math::primitives::Sphere::new(0.12))),
                MeshMaterial3d(flash_materials.add(FlashMaterial {
                    params: FlashMaterialUniform {
                        color: Color::srgb(1.0, 0.16, 0.16).to_linear().to_vec4(),
                        alpha: 0.0,
                        camera_pos: Vec3::ZERO,
                        _pad0: 0.0,
                    },
                })),
                Visibility::Hidden,
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.7)),
            ))
            .id();
        let beam = commands
            .spawn((
                BeamerBeamVisual,
                Mesh3d(meshes.add(bevy::math::primitives::Cuboid::new(0.08, 0.08, 1.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    emissive: LinearRgba::new(24.0, 0.6, 0.6, 1.0),
                    base_color: Color::srgba(1.0, 0.2, 0.2, 0.8),
                    alpha_mode: AlphaMode::Add,
                    unlit: true,
                    ..default()
                })),
                Visibility::Hidden,
                Transform::default(),
            ))
            .id();
        commands.entity(entity).add_child(charge);
        commands.entity(entity).add_child(beam);
        commands
            .entity(entity)
            .insert(BeamerVisualRefs { charge, beam });
    }
}

#[cfg(feature = "client")]
fn sync_visuals(
    beamers: Query<(&BeamerComponent, &BeamerVisualRefs, &GlobalTransform)>,
    tick: Res<common::tick::Ticker>,
    mut visuals: ParamSet<(
        Query<(&mut Transform, &mut Visibility), With<BeamerChargeVisual>>,
        Query<(&mut Transform, &mut Visibility), With<BeamerBeamVisual>>,
    )>,
    charge_materials: Query<&MeshMaterial3d<FlashMaterial>, With<BeamerChargeVisual>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut flash_materials: ResMut<Assets<FlashMaterial>>,
) {
    let camera_pos = camera
        .single()
        .map(GlobalTransform::translation)
        .unwrap_or(Vec3::ZERO);
    for (beamer, refs, global_transform) in &beamers {
        if let Ok((mut transform, mut visibility)) = visuals.p0().get_mut(refs.charge) {
            if beamer.phase == BeamPhase::Charging {
                let elapsed_ticks = tick
                    .tick
                    .saturating_sub(beamer.phase_started_tick)
                    .saturating_add(1);
                let progress = (elapsed_ticks as f32 / CHARGE_TICKS as f32).clamp(0.0, 1.0);
                transform.scale = Vec3::ONE;
                *visibility = Visibility::Inherited;
                if let Ok(handle) = charge_materials.get(refs.charge) {
                    update_flash_material(
                        handle,
                        &mut flash_materials,
                        Color::srgb(1.0, 0.16, 0.16).to_linear(),
                        18.0 * progress,
                        18.0,
                        camera_pos,
                    );
                }
            } else {
                transform.scale = Vec3::ONE;
                *visibility = Visibility::Hidden;
                if let Ok(handle) = charge_materials.get(refs.charge) {
                    update_flash_material(
                        handle,
                        &mut flash_materials,
                        Color::srgb(1.0, 0.16, 0.16).to_linear(),
                        0.0,
                        18.0,
                        camera_pos,
                    );
                }
            }
        }
        if let Ok((mut transform, mut visibility)) = visuals.p1().get_mut(refs.beam) {
            if beamer.phase != BeamPhase::Beaming || beamer.beam_dir == Vec3::ZERO {
                *visibility = Visibility::Hidden;
                continue;
            }
            let local_dir = global_transform
                .affine()
                .inverse()
                .transform_vector3(beamer.beam_dir)
                .normalize_or_zero();
            transform.translation = local_dir * (RANGE * 0.5);
            transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, local_dir);
            transform.scale = Vec3::new(1.0, 1.0, RANGE);
            *visibility = Visibility::Inherited;
        }
    }
}
