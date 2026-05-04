use std::collections::{HashMap, HashSet};

use bevy::{ecs::system::SystemState, prelude::*};
use common::slow_update::{SEMI_SLOW_UPDATE_FREQUENCY, SemiSlowUpdate, SlowUpdate};
use net::{
    message::MsgType,
    quic::{Channel, ConnectionId, QuicManager, SendTarget},
};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};

use crate::{
    AuthoritySet,
    health::Health,
    level::{ScriptZone, parented_world_pose},
    pawn::PlayerRegistry,
};

#[cfg(feature = "client")]
use crate::{messages, pawn::Possessed};

#[derive(Clone, Debug, Reflect, Default, PartialEq, Eq)]
pub enum ZoneEffectKind {
    #[default]
    Safe,
    OutOfBounds,
}

#[derive(Clone, Debug, Reflect, Default, PartialEq, Eq)]
pub enum ZoneEffectRegion {
    #[default]
    Inside,
    Outside,
}

#[derive(Component, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct ZoneEffect {
    pub kind: ZoneEffectKind,
    pub region: ZoneEffectRegion,
    pub priority: i32,
    pub countdown_secs: f32,
    pub damage_fraction_per_sec: f32,
}

impl Default for ZoneEffect {
    fn default() -> Self {
        Self {
            kind: ZoneEffectKind::Safe,
            region: ZoneEffectRegion::Inside,
            priority: 0,
            countdown_secs: 0.0,
            damage_fraction_per_sec: 0.0,
        }
    }
}

#[derive(Resource, Default)]
struct ZoneEffectRuntime {
    players: HashMap<Entity, PlayerZoneState>,
}

#[derive(Clone, Copy, Default)]
struct PlayerZoneState {
    active_zone: Option<Entity>,
    countdown_remaining: f32,
    last_shown_second: Option<i32>,
}

#[derive(Clone, Copy)]
struct ActiveZone {
    entity: Entity,
    priority: i32,
    countdown_secs: f32,
    damage_fraction_per_sec: f32,
}

#[derive(Default)]
struct ZoneOverlap {
    safe: bool,
    harmful: Option<ActiveZone>,
}

pub struct ZoneEffectsPlugin;

impl Plugin for ZoneEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ZoneEffectKind>();
        app.register_type::<ZoneEffectRegion>();
        app.register_type::<ZoneEffect>();
        app.init_resource::<ZoneEffectRuntime>();
        app.add_systems(SemiSlowUpdate, tick_zone_effects.in_set(AuthoritySet::Level));
        app.add_systems(SlowUpdate, tick_zone_messages.in_set(AuthoritySet::Level));
    }
}

fn tick_zone_effects(world: &mut World) {
    let mut runtime = world.remove_resource::<ZoneEffectRuntime>().unwrap_or_default();
    let players = collect_player_entities(world);
    let player_set: HashSet<Entity> = players.iter().copied().collect();
    runtime
        .players
        .retain(|entity, _| player_set.contains(entity) && world.entities().contains(*entity));

    let mut overlaps: HashMap<Entity, ZoneOverlap> =
        players.iter().copied().map(|entity| (entity, ZoneOverlap::default())).collect();

    {
        let mut state: SystemState<(
            Query<(Entity, &ScriptZone, &ZoneEffect, &Transform, Option<&ChildOf>)>,
            Query<&Transform>,
            Query<&ChildOf>,
            Query<&RigidBodyHandleComponent>,
            Res<PhysicsWorld>,
            Query<&mut Health>,
        )> = SystemState::new(world);
        let (zones, parent_transforms, parent_parents, parent_bodies, physics, mut health_q) =
            state.get_mut(world);

        for (zone_entity, zone, effect, transform, child_of) in &zones {
            let (position, rotation) = parented_world_pose(
                transform,
                child_of,
                &parent_transforms,
                &parent_parents,
                &parent_bodies,
                &physics,
            );
            for entity in &players {
                let player_pos = physics.body_pos(*entity);
                let inside = player_pos.is_some_and(|player_pos| {
                    zone.shape.contains_point(1.0, position, rotation, player_pos)
                });
                let applies = match effect.region {
                    ZoneEffectRegion::Inside => inside,
                    ZoneEffectRegion::Outside => !inside,
                };
                if !applies {
                    continue;
                }
                let entry = overlaps.entry(*entity).or_default();
                match effect.kind {
                    ZoneEffectKind::Safe => entry.safe = true,
                    ZoneEffectKind::OutOfBounds => {
                        let replace = entry
                            .harmful
                            .is_none_or(|current| effect.priority >= current.priority);
                        if replace {
                            entry.harmful = Some(ActiveZone {
                                entity: zone_entity,
                                priority: effect.priority,
                                countdown_secs: effect.countdown_secs.max(0.0),
                                damage_fraction_per_sec: effect.damage_fraction_per_sec.max(0.0),
                            });
                        }
                    }
                }
            }
        }

        let damage_step = 1.0 / SEMI_SLOW_UPDATE_FREQUENCY as f32;
        for entity in players {
            let Some(overlap) = overlaps.remove(&entity) else {
                runtime.players.remove(&entity);
                continue;
            };
            if overlap.safe {
                runtime.players.remove(&entity);
                continue;
            }
            let Some(active) = overlap.harmful else {
                runtime.players.remove(&entity);
                continue;
            };
            let state = runtime.players.entry(entity).or_default();
            if state.active_zone != Some(active.entity) {
                *state = PlayerZoneState {
                    active_zone: Some(active.entity),
                    countdown_remaining: active.countdown_secs,
                    last_shown_second: None,
                };
            }
            if state.countdown_remaining > 0.0 {
                state.countdown_remaining = (state.countdown_remaining - damage_step).max(0.0);
            }
            if state.countdown_remaining <= 0.0 && active.damage_fraction_per_sec > 0.0 {
                if let Ok(mut health) = health_q.get_mut(entity) {
                    health.apply_percent_damage(active.damage_fraction_per_sec * damage_step);
                }
            }
        }
    }

    world.insert_resource(runtime);
}

fn tick_zone_messages(world: &mut World) {
    let mut runtime = world.remove_resource::<ZoneEffectRuntime>().unwrap_or_default();
    let mut outbound = Vec::new();
    runtime.players.retain(|entity, state| {
        if !world.entities().contains(*entity) {
            return false;
        }
        let Some(zone) = state.active_zone else {
            state.last_shown_second = None;
            return true;
        };
        if !world.entities().contains(zone) || state.countdown_remaining <= 0.0 {
            state.last_shown_second = None;
            return true;
        }
        let seconds = state.countdown_remaining.ceil() as i32;
        if state.last_shown_second != Some(seconds) {
            outbound.push((*entity, format!("Return to combat. {seconds}...")));
            state.last_shown_second = Some(seconds);
        }
        true
    });
    for (entity, text) in outbound {
        send_zone_message(world, entity, text);
    }
    world.insert_resource(runtime);
}

fn collect_player_entities(world: &mut World) -> Vec<Entity> {
    let mut players = Vec::new();
    if let Some(registry) = world.get_resource::<PlayerRegistry>() {
        for (_, (entity, _)) in registry.controlled_entries() {
            if !players.contains(entity) {
                players.push(*entity);
            }
        }
    }
    #[cfg(feature = "client")]
    {
        let mut possessed = world.query_filtered::<Entity, With<Possessed>>();
        for entity in possessed.iter(world) {
            if !players.contains(&entity) {
                players.push(entity);
            }
        }
    }
    players
}

fn connection_for_player(world: &World, entity: Entity) -> Option<ConnectionId> {
    let registry = world.get_resource::<PlayerRegistry>()?;
    registry
        .controlled_entries()
        .find_map(|(conn_id, (controlled, _))| (*controlled == entity).then_some(*conn_id))
}

fn send_zone_message(world: &mut World, entity: Entity, text: String) {
    if let Some(conn_id) = connection_for_player(world, entity) {
        if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &MsgType::OnscreenMessage(text),
            );
            return;
        }
    }
    #[cfg(feature = "client")]
    if world.get::<Possessed>(entity).is_some() {
        messages::push_world(world, text);
    }
}
