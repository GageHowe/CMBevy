use std::collections::HashMap;

use bevy::prelude::*;
use common::{
    config::{DEFAULT_GC_SOFT_CAP, GC_OVERFLOW_STEP, MAX_GC_OBJECTS},
    slow_update::SlowUpdate,
};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_pos};

use crate::AuthoritySystems;

#[derive(Component, Clone, Copy)]
pub struct WorldObjectGc {
    pub remaining_secs: f32,
    pub max_secs: f32,
    pub last_relevant_secs: f32,
}

impl WorldObjectGc {
    pub const fn new(max_secs: f32) -> Self {
        Self {
            remaining_secs: max_secs,
            max_secs,
            last_relevant_secs: 0.0,
        }
    }
}

#[derive(Component)]
pub(crate) struct SpawnerGc {
    pub spawner: Entity,
}

const GC_DT_SECS: f32 = 1.0;
const GC_KEEP_DISTANCE: f32 = 90.0;
const GC_LOCAL_CELL_SIZE: f32 = 50.0;
const GC_LOCAL_CAP: f32 = 8.0;
const GC_MIN_RELEVANCE: f32 = 0.05;
const GC_MIN_DRAIN_PER_SEC: f32 = 0.15;
const GC_MAX_AGE_MULTIPLIER: f32 = 2.0;

pub struct WorldGcPlugin;

impl Plugin for WorldGcPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SlowUpdate,
            cleanup_world_gc_entities.in_set(AuthoritySystems),
        );
    }
}

#[derive(bevy::ecs::system::SystemParam)]
struct WorldGcState<'w, 's> {
    pawns: Query<
        'w,
        's,
        &'static RigidBodyHandleComponent,
        Or<(
            With<crate::pawn::BipedPawnComponent>,
            With<crate::pawn::VehicleComponent>,
        )>,
    >,
    vehicles: Query<'w, 's, &'static crate::pawn::VehicleComponent>,
    mounts: Query<'w, 's, &'static crate::pawn::CharacterMount>,
    spawners: Query<'w, 's, &'static mut crate::level::SpawnerRuntime>,
    map_meta: Option<Res<'w, crate::level::MapMeta>>,
}

impl WorldGcState<'_, '_> {
    fn pawn_positions(&self, physics: &PhysicsWorld) -> Vec<Vec3> {
        self.pawns
            .iter()
            .filter_map(|body| physics.rigid_body_set.get(body.0).map(rb_pos))
            .collect()
    }

    fn soft_cap(&self) -> usize {
        self.map_meta
            .as_ref()
            .and_then(|meta| meta.gc_soft_cap)
            .unwrap_or(DEFAULT_GC_SOFT_CAP)
            .clamp(1, MAX_GC_OBJECTS)
    }

    fn is_in_use(&self, entity: Entity) -> bool {
        self.vehicles
            .get(entity)
            .ok()
            .and_then(|_| self.mounts.get(entity).ok())
            .is_some_and(|mount| mount.occupant.is_some())
    }

    fn notify_spawner_despawn(&mut self, spawner_gc: Option<&SpawnerGc>, entity: Entity) {
        let Some(spawner_gc) = spawner_gc else {
            return;
        };
        let Ok(mut spawner) = self.spawners.get_mut(spawner_gc.spawner) else {
            return;
        };
        if spawner.active_entity != Some(entity) {
            return;
        }
        spawner.active_entity = None;
        spawner.respawn_timer_secs = spawner.respawn_delay_secs;
    }
}

#[derive(Clone, Copy)]
struct GcSnapshot {
    entity: Entity,
    pos: Vec3,
    protected: bool,
}

#[derive(Clone, Copy)]
struct CullCandidate {
    entity: Entity,
    score: f32,
}

fn gc_cell(pos: Vec3) -> IVec3 {
    (pos / GC_LOCAL_CELL_SIZE).floor().as_ivec3()
}

fn nearby_gc_count(counts: &HashMap<IVec3, usize>, cell: IVec3) -> usize {
    let mut total = 0;
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                total += counts
                    .get(&(cell + IVec3::new(x, y, z)))
                    .copied()
                    .unwrap_or(0);
            }
        }
    }
    total
}

fn cleanup_world_gc_entities(
    physics: Res<PhysicsWorld>,
    mut state: WorldGcState,
    mut gc_q: Query<
        (
            Entity,
            &RigidBodyHandleComponent,
            Option<&SpawnerGc>,
            &mut WorldObjectGc,
        ),
        With<WorldObjectGc>,
    >,
    mut commands: Commands,
) {
    let pawn_positions = state.pawn_positions(&physics);
    let soft_cap = state.soft_cap();
    let mut snapshots = Vec::new();

    for (entity, body, _spawner_gc, _gc) in &mut gc_q {
        let Some(rb) = physics.rigid_body_set.get(body.0) else {
            continue;
        };
        if !rb.is_enabled() {
            continue;
        }
        snapshots.push(GcSnapshot {
            entity,
            pos: rb_pos(rb),
            protected: state.is_in_use(entity),
        });
    }

    let enabled_gc_count = snapshots.len();
    let overflow_multiplier =
        1.0 + enabled_gc_count.saturating_sub(soft_cap) as f32 / GC_OVERFLOW_STEP as f32;
    let mut cell_counts = HashMap::new();
    for snapshot in &snapshots {
        *cell_counts.entry(gc_cell(snapshot.pos)).or_insert(0usize) += 1;
    }
    let mut cull_candidates = Vec::new();

    for (entity, body, spawner_gc, mut gc) in &mut gc_q {
        let Some(rb) = physics.rigid_body_set.get(body.0) else {
            continue;
        };

        if !rb.is_enabled() || state.is_in_use(entity) {
            gc.remaining_secs = gc.max_secs;
            gc.last_relevant_secs = 0.0;
            continue;
        }

        let pos = rb_pos(rb);
        let nearest_pawn_dist = pawn_positions
            .iter()
            .map(|pawn| pawn.distance(pos))
            .min_by(|a, b| a.total_cmp(b))
            .unwrap_or(f32::INFINITY);
        let distance_relevance = (1.0 - nearest_pawn_dist / GC_KEEP_DISTANCE)
            .clamp(0.0, 1.0)
            .max(GC_MIN_RELEVANCE);
        if distance_relevance >= 0.5 {
            gc.last_relevant_secs = 0.0;
        } else {
            gc.last_relevant_secs += GC_DT_SECS;
        }
        let local_count = nearby_gc_count(&cell_counts, gc_cell(pos)).saturating_sub(1) as f32;
        let crowding_factor = 1.0 / (1.0 + local_count / GC_LOCAL_CAP);
        let age_multiplier = 1.0
            + (gc.last_relevant_secs / gc.max_secs).clamp(0.0, 1.0) * (GC_MAX_AGE_MULTIPLIER - 1.0);
        let drain = GC_DT_SECS
            * GC_MIN_DRAIN_PER_SEC.max(1.0 - distance_relevance * crowding_factor)
            * age_multiplier
            * overflow_multiplier;
        gc.remaining_secs = (gc.remaining_secs - drain).max(0.0);
        if gc.remaining_secs > 0.0 {
            cull_candidates.push(CullCandidate {
                entity,
                score: distance_relevance * crowding_factor - gc.last_relevant_secs / gc.max_secs,
            });
            continue;
        }

        state.notify_spawner_despawn(spawner_gc, entity);
        eprintln!("gc despawned world entity {entity}");
        commands.entity(entity).despawn();
    }

    let overflow = enabled_gc_count.saturating_sub(MAX_GC_OBJECTS);
    if overflow == 0 {
        return;
    }

    cull_candidates.sort_by(|a, b| a.score.total_cmp(&b.score));
    let protected: HashMap<Entity, bool> = snapshots
        .iter()
        .map(|snapshot| (snapshot.entity, snapshot.protected))
        .collect();
    let mut culled = 0usize;
    for candidate in cull_candidates {
        if culled >= overflow {
            break;
        }
        if protected.get(&candidate.entity).copied().unwrap_or(false) {
            continue;
        }
        if let Ok((_, _, spawner_gc, _)) = gc_q.get_mut(candidate.entity) {
            state.notify_spawner_despawn(spawner_gc, candidate.entity);
        }
        eprintln!("gc hard-cap despawned world entity {}", candidate.entity);
        commands.entity(candidate.entity).despawn();
        culled += 1;
    }
}
