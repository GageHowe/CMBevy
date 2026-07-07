#[cfg(feature = "client")]
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;
use net::quic::{Channel, QuicManager, SendTarget};
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::*;
#[cfg(feature = "client")]
use crate::flash::{FlashMaterial, FlashMaterialUniform, update_flash_material};
use crate::{
    NetworkEntityMap,
    health::{DamageCause, Health, LastDamageSource, attribute_damage},
    pawn::PlayerRegistry,
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
        #[cfg(not(feature = "client"))]
        _app.add_systems(
            FixedUpdate,
            drive_authoritative_beams
                .before(super::tick_weapon_state)
                .in_set(super::SimulateItemSet),
        );
    }
}

#[cfg(not(feature = "client"))]
fn drive_authoritative_beams(
    mut beamers: Query<(
        Entity,
        &net::message::NetworkID,
        &WeaponFireInput,
        &mut BeamerComponent,
        &mut WeaponState,
        &WeaponConfig,
    )>,
    registry: Res<PlayerRegistry>,
    world: Res<PhysicsWorld>,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
    mut quic: ResMut<QuicManager>,
    mut commands: Commands,
) {
    for (entity, net_id, input, mut beam, mut state, config) in &mut beamers {
        let target = registry
            .conn_id_for_character(input.shooter)
            .map_or(SendTarget::All, SendTarget::AllExcept);
        if input.reload_pressed || !input.want_fire {
            if beam.phase != BeamPhase::Idle {
                beam.phase = BeamPhase::Idle;
                beam.beam_dir = Vec3::ZERO;
                state.cooldown_ticks = COOLDOWN_TICKS;
                quic.send(
                    target,
                    Channel::Ordered,
                    &net::message::MsgType::EndBeam(net_id.clone()),
                );
            }
            if input.reload_pressed {
                super::start_reload(&mut state, config);
            }
            commands.entity(entity).remove::<WeaponFireInput>();
            continue;
        }
        if beam.phase == BeamPhase::Idle && super::can_fire(&state) {
            beam.phase = BeamPhase::Charging;
            beam.phase_started_tick = input.tick;
            quic.send(
                target.clone(),
                Channel::Ordered,
                &net::message::MsgType::StartBeamCharge(net_id.clone()),
            );
        } else if beam.phase == BeamPhase::Charging
            && input.tick.saturating_sub(beam.phase_started_tick) + 1 >= CHARGE_TICKS as u64
        {
            beam.phase = BeamPhase::Beaming;
            beam.damage_tick_accum = DAMAGE_INTERVAL_TICKS - 1;
            quic.send(
                target.clone(),
                Channel::Ordered,
                &net::message::MsgType::StartBeam {
                    weapon: net_id.clone(),
                    origin: input.origin,
                    dir: input.aim_dir,
                },
            );
        }
        beam.beam_origin = input.origin;
        beam.beam_dir = input.aim_dir.normalize_or_zero();
        if beam.phase == BeamPhase::Beaming {
            beam.damage_tick_accum += 1;
            if beam.damage_tick_accum >= DAMAGE_INTERVAL_TICKS && state.ammo_in_mag > 0 {
                beam.damage_tick_accum = 0;
                state.ammo_in_mag -= 1;
                if let Some((hit, _)) =
                    beam_hit(&world, input.origin, beam.beam_dir, Some(input.shooter))
                    && let Ok(mut health) = health_q.get_mut(hit)
                {
                    attribute_damage(
                        &mut last_damage_q,
                        hit,
                        Some(input.shooter),
                        DamageCause::Projectile,
                    );
                    health.apply_damage(DAMAGE_PER_TICK);
                }
                quic.send(
                    target.clone(),
                    Channel::Unordered,
                    &net::message::MsgType::BeamHitReport {
                        weapon: net_id.clone(),
                        origin: input.origin,
                        dir: beam.beam_dir,
                        target: None,
                    },
                );
                if state.ammo_in_mag == 0 {
                    beam.phase = BeamPhase::Idle;
                    super::start_reload(&mut state, config);
                    quic.send(
                        target,
                        Channel::Ordered,
                        &net::message::MsgType::EndBeam(net_id.clone()),
                    );
                }
            }
        }
        commands.entity(entity).remove::<WeaponFireInput>();
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
    decorate_projectile: None,
    projectile_behavior: None,
};

#[cfg(feature = "client")]
fn update_beamer(
    weapon: &mut BeamerComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    input: &WeaponFireInput,
    state: &mut WeaponState,
    config: &WeaponConfig,
    multiplayer: bool,
) {
    if input.reload_pressed {
        end_local_beam(weapon, state, true);
        super::start_reload(state, config);
        return;
    }

    if !input.want_fire {
        end_local_beam(weapon, state, true);
        return;
    }

    if weapon.phase == BeamPhase::Idle {
        if !super::can_fire(state) {
            return;
        }
        weapon.phase = BeamPhase::Charging;
        weapon.phase_started_tick = input.tick;
        weapon.damage_tick_accum = 0;
        weapon.beam_origin = input.origin;
        weapon.beam_dir = input.aim_dir.normalize_or_zero();
        return;
    }

    if weapon.phase == BeamPhase::Charging {
        weapon.beam_origin = input.origin;
        weapon.beam_dir = input.aim_dir.normalize_or_zero();
        if input.tick.saturating_sub(weapon.phase_started_tick) + 1 < CHARGE_TICKS as u64 {
            return;
        }
        weapon.phase = BeamPhase::Beaming;
        weapon.phase_started_tick = input.tick;
        weapon.damage_tick_accum = DAMAGE_INTERVAL_TICKS - 1;
    }

    if weapon.phase != BeamPhase::Beaming {
        return;
    }

    weapon.beam_origin = input.origin;
    weapon.beam_dir = input.aim_dir.normalize_or_zero();
    if weapon.beam_dir == Vec3::ZERO {
        return;
    }

    weapon.damage_tick_accum += 1;
    if weapon.damage_tick_accum < DAMAGE_INTERVAL_TICKS {
        return;
    }
    weapon.damage_tick_accum = 0;

    if state.ammo_in_mag == 0 {
        end_local_beam(weapon, state, true);
        super::start_reload(state, config);
        return;
    }
    state.ammo_in_mag -= 1;
    if state.ammo_in_mag == 0 {
        super::start_reload(state, config);
    }

    #[cfg(feature = "client")]
    let hit = beam_hit(world, input.origin, weapon.beam_dir, Some(input.shooter));
    #[cfg(feature = "client")]
    if !multiplayer {
        let shooter = Some(input.shooter);
        commands.queue(move |world: &mut World| {
            apply_singleplayer_beam_hit(world, shooter, hit.map(|(entity, _)| entity));
        });
    }

    if state.ammo_in_mag == 0 {
        end_local_beam(weapon, state, false);
    }
}

#[cfg(feature = "client")]
pub fn drive_beamers(
    mut weapons: Query<(
        Entity,
        &mut BeamerComponent,
        &mut WeaponState,
        &WeaponConfig,
        &WeaponFireInput,
    )>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    quic: Option<Res<QuicManager>>,
) {
    for (entity, mut beam, mut state, config, input) in &mut weapons {
        update_beamer(
            &mut beam,
            &mut world,
            &mut commands,
            input,
            &mut state,
            config,
            quic.is_some(),
        );
        commands.entity(entity).remove::<WeaponFireInput>();
    }
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
fn beam_len(world: &PhysicsWorld, origin: Vec3, dir: Vec3, exclude: &[Entity]) -> f32 {
    world
        .cast_ray(origin, dir, RANGE, exclude)
        .map(|(_, toi)| toi)
        .unwrap_or(RANGE)
}

#[cfg(feature = "client")]
fn end_local_beam(beam: &mut BeamerComponent, state: &mut WeaponState, apply_cooldown: bool) {
    if beam.phase == BeamPhase::Idle {
        return;
    }
    beam.phase = BeamPhase::Idle;
    beam.damage_tick_accum = 0;
    beam.beam_dir = Vec3::ZERO;
    if apply_cooldown {
        state.cooldown_ticks = COOLDOWN_TICKS;
    }
}

#[cfg(feature = "client")]
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
    beamers: Query<(
        Entity,
        &BeamerComponent,
        &BeamerVisualRefs,
        &GlobalTransform,
    )>,
    tick: Res<common::tick::Ticker>,
    world: Res<PhysicsWorld>,
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
    for (entity, beamer, refs, global_transform) in &beamers {
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
            let len = beam_len(&world, beamer.beam_origin, beamer.beam_dir, &[entity]);
            let local_dir = global_transform
                .affine()
                .inverse()
                .transform_vector3(beamer.beam_dir)
                .normalize_or_zero();
            transform.translation = local_dir * (len * 0.5);
            transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, local_dir);
            transform.scale = Vec3::new(1.0, 1.0, len);
            *visibility = Visibility::Inherited;
        }
    }
}
