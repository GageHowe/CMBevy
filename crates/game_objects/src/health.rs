use crate::dispatch_game_object_on_death;
use crate::mode::ModeConfig;
use crate::pawn::biped::WeaponSlots;
use crate::pawn::{HeldWeaponMap, PendingRespawns, PlayerRegistry};
use bevy::prelude::*;
use common::GameObjectKind;
use common::NetworkID;
use common::game_state::GameState;
use net::message::MsgType;
use net::quic::{Channel, QuicManager, SendTarget};
use physics::physics_world::{PhysicsWorld, rb_pos, step_physics};
use std::collections::HashMap;

const BIPED_REGEN_PER_SEC: f32 = 4.0;
const DAMAGE_ATTRIBUTION_WINDOW_SECS: f32 = 6.0;

pub struct HealthPlugin;
impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                apply_collision_damage.after(step_physics),
                regenerate_biped_health,
                age_last_damage_sources,
            )
                .chain()
                .run_if(death_authority),
        );
        app.add_systems(
            FixedUpdate,
            handle_deaths.after(step_physics).run_if(death_authority),
        );
    }
}

fn death_authority(state: Option<Res<State<GameState>>>) -> bool {
    #[cfg(feature = "client")]
    {
        state.is_some_and(|s| *s.get() == GameState::SinglePlayer)
    }
    #[cfg(not(feature = "client"))]
    {
        let _ = state;
        true
    }
}

#[derive(Component, Clone, Copy)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

/// Tracks the most recent gameplay-owned attacker for a health-bearing entity so match scripts
/// can resolve kills without re-implementing attribution logic in Lua.
#[derive(Component, Clone, Copy, Default, Reflect)]
pub struct LastDamageSource {
    pub attacker: Option<Entity>,
    pub age_secs: f32,
}

impl LastDamageSource {
    pub fn resolved_attacker(self) -> Option<Entity> {
        (self.age_secs <= DAMAGE_ATTRIBUTION_WINDOW_SECS)
            .then_some(self.attacker)
            .flatten()
    }
}

/// Deferred script kill callbacks drained after authoritative death handling has finished.
#[derive(Resource, Default)]
pub struct PendingPlayerKills(pub Vec<(Entity, Option<Entity>)>);

/// Dead player entities stay in the registry until kill callbacks run so scripts can still
/// mutate their player numbers during `on_player_killed`.
#[derive(Resource, Default)]
pub struct PendingPlayerRemovals(pub Vec<Entity>);

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    /// Apply damage; returns `true` if this damage killed the entity.
    pub fn apply_damage(&mut self, amount: f32) -> bool {
        self.current = (self.current - amount).max(0.0);
        self.current <= 0.0
    }
}

#[derive(Component, Clone, Copy)]
pub struct CollisionDamageConfig {
    pub threshold_per_mass: f32,
    pub min_threshold: f32,
    pub damage_scale: f32,
    pub max_damage_per_hit: Option<f32>,
}

impl Default for CollisionDamageConfig {
    fn default() -> Self {
        Self {
            // Default to a mass-scaled threshold so heavier objects don't take the same landing
            // damage as light ones from identical support/contact impulses.
            threshold_per_mass: 40.0,
            min_threshold: 40.0,
            damage_scale: 3.0,
            max_damage_per_hit: None,
        }
    }
}

impl CollisionDamageConfig {
    fn damage_from_impulse(self, impulse: f32, mass: f32) -> f32 {
        let threshold = (mass * self.threshold_per_mass).max(self.min_threshold);
        let mut damage = (impulse - threshold).max(0.0) * self.damage_scale;
        if let Some(max_damage) = self.max_damage_per_hit {
            damage = damage.min(max_damage);
        }
        damage
    }
}

/// Applies damage to any entity with Health + a rigidbody based on collision impulse.
/// Rapier's contact impulse already reflects the force imparted by the collision, so
/// damage only needs one path regardless of what the body hit.
/// Must run after step_physics.
pub fn apply_collision_damage(
    world: Res<PhysicsWorld>,
    has_health_q: Query<Option<&CollisionDamageConfig>, With<Health>>,
    mut health_q: Query<&mut Health>,
) {
    let mut damage_map: HashMap<Entity, f32> = HashMap::new();
    for pair in world.narrow_phase.contact_pairs() {
        if !pair.has_any_active_contact() {
            continue;
        }
        let impulse: f32 = pair
            .manifolds
            .iter()
            .flat_map(|m| m.points.iter())
            .map(|p| p.data.impulse)
            .sum();
        if impulse <= 0.0 {
            continue;
        }
        for rb_h in [pair.collider1, pair.collider2]
            .into_iter()
            .filter_map(|collider| world.collider_set.get(collider).and_then(|c| c.parent()))
        {
            let Some(rb) = world.rigid_body_set.get(rb_h) else {
                continue;
            };
            if !rb.is_dynamic() {
                continue;
            }
            if let Some(&entity) = world.handle_to_entity.get(&rb_h) {
                let Ok(config) = has_health_q.get(entity) else {
                    continue;
                };
                let damage = config
                    .copied()
                    .unwrap_or_default()
                    .damage_from_impulse(impulse, rb.mass());
                if damage <= 0.0 {
                    continue;
                }
                info!("collision impulse: {impulse:.2}  damage: {damage:.1}");
                *damage_map.entry(entity).or_default() += damage;
            }
        }
    }

    for (entity, damage) in damage_map {
        if let Ok(mut health) = health_q.get_mut(entity) {
            health.apply_damage(damage);
        }
    }
}

fn age_last_damage_sources(time: Res<Time<Fixed>>, mut q: Query<&mut LastDamageSource>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut last_damage in &mut q {
        if last_damage.attacker.is_none() {
            continue;
        }
        last_damage.age_secs += dt;
        if last_damage.age_secs > DAMAGE_ATTRIBUTION_WINDOW_SECS {
            last_damage.attacker = None;
        }
    }
}

fn regenerate_biped_health(
    time: Res<Time<Fixed>>,
    mut health_q: Query<(&mut Health, &GameObjectKind)>,
) {
    let heal = BIPED_REGEN_PER_SEC * time.delta_secs();
    if heal <= 0.0 {
        return;
    }
    for (mut health, kind) in &mut health_q {
        if *kind != GameObjectKind::Biped || health.current <= 0.0 || health.current >= health.max {
            continue;
        }
        health.current = (health.current + heal).min(health.max);
    }
}

pub fn handle_deaths(world: &mut World) {
    let dead: Vec<(Entity, Option<common::GameObjectKind>, Option<NetworkID>)> = {
        let mut q = world.query_filtered::<(
            Entity,
            &Health,
            Option<&common::GameObjectKind>,
            Option<&NetworkID>,
        ), Changed<Health>>();
        q.iter(world)
            .filter(|(_, health, _, _)| health.current <= 0.0)
            .map(|(entity, _, kind, net_id)| (entity, kind.cloned(), net_id.cloned()))
            .collect()
    };

    for (entity, kind, net_id) in dead {
        let weapon_drops = if matches!(kind, Some(common::GameObjectKind::Biped)) {
            collect_weapon_drops(entity, world)
        } else {
            Vec::new()
        };
        let vehicle_eject = if matches!(kind, Some(common::GameObjectKind::Spaceship)) {
            collect_vehicle_eject(entity, world)
        } else {
            None
        };

        let should_despawn = kind
            .clone()
            .is_none_or(|kind| run_death_callback(kind, entity, world));
        if !should_despawn || !world.entities().contains(entity) {
            continue;
        }

        if matches!(kind, Some(common::GameObjectKind::Biped)) {
            handle_biped_death(entity, &weapon_drops, world);
        }
        if let Some((biped_entity, biped_net_id)) = vehicle_eject {
            handle_spaceship_death(biped_entity, biped_net_id, world);
        }

        if let Some(net_id) = net_id {
            if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
                quic.send(
                    SendTarget::All,
                    Channel::Ordered,
                    &MsgType::DespawnCommand(net_id),
                );
            }
        }
        world.entity_mut(entity).despawn();
    }
}

fn run_death_callback(kind: common::GameObjectKind, entity: Entity, world: &mut World) -> bool {
    dispatch_game_object_on_death(kind, entity, world)
}

fn collect_weapon_drops(entity: Entity, world: &World) -> Vec<(NetworkID, Vec3)> {
    let drop_pos = {
        let physics = world.resource::<PhysicsWorld>();
        physics
            .entity_to_handle
            .get(&entity)
            .and_then(|&h| physics.rigid_body_set.get(h))
            .map(rb_pos)
            .unwrap_or(Vec3::ZERO)
    };
    world
        .get::<WeaponSlots>(entity)
        .map(|slots| {
            [slots.primary.clone(), slots.pocket.clone()]
                .into_iter()
                .filter_map(|(net_id, _weapon_entity)| Some((net_id?, drop_pos)))
                .collect()
        })
        .unwrap_or_default()
}

fn handle_biped_death(entity: Entity, weapon_drops: &[(NetworkID, Vec3)], world: &mut World) {
    let conn_id = {
        let Some(registry) = world.get_resource::<PlayerRegistry>() else {
            return;
        };
        let Some(conn_id) = registry.conn_id_for_entity(entity) else {
            return;
        };
        conn_id
    };
    let owner_net_id = world.get::<NetworkID>(entity).cloned();
    let killer = world
        .get::<LastDamageSource>(entity)
        .copied()
        .and_then(LastDamageSource::resolved_attacker);

    if let Some(mut held_map) = world.get_resource_mut::<HeldWeaponMap>() {
        for (weapon_id, _) in weapon_drops.iter() {
            held_map.0.remove(weapon_id);
        }
    }
    if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
        for (weapon_id, drop_pos) in weapon_drops.iter().cloned() {
            if let Some(ref net_id) = owner_net_id {
                quic.send(
                    SendTarget::All,
                    Channel::Ordered,
                    &MsgType::WeaponDrop(weapon_id, net_id.clone(), drop_pos),
                );
            }
        }
    }

    if let Some(mut pending_kills) = world.get_resource_mut::<PendingPlayerKills>() {
        pending_kills.0.push((entity, killer));
    }
    let mut deferred_removal = false;
    if let Some(mut pending_removals) = world.get_resource_mut::<PendingPlayerRemovals>() {
        pending_removals.0.push(entity);
        deferred_removal = true;
    }
    if !deferred_removal && let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() {
        let _ = registry.remove_by_entity(entity);
    }
    let respawn_delay = world
        .get_resource::<ModeConfig>()
        .map_or(common::config::RESPAWN_DELAY_SECS, |cfg| cfg.respawn_delay);
    if let Some(mut pending_respawns) = world.get_resource_mut::<PendingRespawns>() {
        pending_respawns
            .0
            .insert(conn_id, (respawn_delay, common::GameObjectKind::Biped));
    }
}

pub fn attribute_damage(
    last_damage_q: &mut Query<&mut LastDamageSource>,
    victim: Entity,
    attacker: Option<Entity>,
) {
    let Ok(mut last_damage) = last_damage_q.get_mut(victim) else {
        return;
    };
    last_damage.attacker = attacker;
    last_damage.age_secs = 0.0;
}

pub fn copy_last_damage_source(world: &mut World, from: Entity, to: Entity) {
    let Some(source) = world.get::<LastDamageSource>(from).copied() else {
        return;
    };
    let Some(mut target) = world.get_mut::<LastDamageSource>(to) else {
        return;
    };
    *target = source;
}

fn collect_vehicle_eject(entity: Entity, world: &mut World) -> Option<(Entity, NetworkID)> {
    world
        .get::<crate::pawn::vehicle::VehicleComponent>(entity)
        .and_then(|vehicle| world.get::<crate::pawn::vehicle::DriverSeat>(vehicle.driver_seat))
        .and_then(|cockpit| cockpit.occupant)
        .and_then(|biped_entity| {
            world
                .get::<NetworkID>(biped_entity)
                .cloned()
                .map(|nid| (biped_entity, nid))
        })
}

fn handle_spaceship_death(biped_entity: Entity, biped_net_id: NetworkID, world: &mut World) {
    let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() else {
        return;
    };
    let Some(conn_id) = registry.conn_id_for_entity(biped_entity) else {
        return;
    };
    registry.set_controlled(conn_id, biped_entity, biped_net_id.clone());
    drop(registry);
    if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::Possess(biped_net_id.clone()),
        );
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::SeatState(biped_net_id, None),
        );
    }
}
