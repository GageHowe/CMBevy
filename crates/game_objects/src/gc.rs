use bevy::prelude::*;
use common::slow_update::SlowUpdate;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_pos};

use crate::AuthoritySystems;

#[derive(Component, Clone, Copy)]
pub struct WorldObjectGc {
    pub remaining_secs: f32,
    pub reset_secs: f32,
}

impl WorldObjectGc {
    pub const fn new(reset_secs: f32) -> Self {
        Self {
            remaining_secs: reset_secs,
            reset_secs,
        }
    }
}

#[derive(Component)]
pub(crate) struct SpawnerGc {
    pub spawner: Entity,
}

const GC_RELEVANT_RADIUS_SQ: f32 = 90.0 * 90.0;
const GC_DT_SECS: f32 = 1.0;
const GC_SOFT_CAP: usize = 48;
const GC_OVERFLOW_STEP: usize = 12;

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
    gc_count: Query<'w, 's, &'static RigidBodyHandleComponent, With<WorldObjectGc>>,
    vehicles: Query<'w, 's, &'static crate::pawn::VehicleComponent>,
    mounts: Query<'w, 's, &'static crate::pawn::CharacterMount>,
    spawners: Query<'w, 's, &'static mut crate::level::SpawnerRuntime>,
}

impl WorldGcState<'_, '_> {
    fn pawn_positions(&self, physics: &PhysicsWorld) -> Vec<Vec3> {
        self.pawns
            .iter()
            .filter_map(|body| physics.rigid_body_set.get(body.0).map(rb_pos))
            .collect()
    }

    fn enabled_gc_count(&self, physics: &PhysicsWorld) -> usize {
        self.gc_count
            .iter()
            .filter(|body| {
                physics
                    .rigid_body_set
                    .get(body.0)
                    .is_some_and(|rb| rb.is_enabled())
            })
            .count()
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
    let decay_scale = 1.0
        + state.enabled_gc_count(&physics).saturating_sub(GC_SOFT_CAP) as f32
            / GC_OVERFLOW_STEP as f32;

    for (entity, body, spawner_gc, mut gc) in &mut gc_q {
        let Some(rb) = physics.rigid_body_set.get(body.0) else {
            continue;
        };

        if !rb.is_enabled() || state.is_in_use(entity) {
            gc.remaining_secs = gc.reset_secs;
            continue;
        }

        let pos = rb_pos(rb);
        if pawn_positions
            .iter()
            .any(|pawn| pawn.distance_squared(pos) <= GC_RELEVANT_RADIUS_SQ)
        {
            gc.remaining_secs = gc.reset_secs;
            continue;
        }

        gc.remaining_secs = (gc.remaining_secs - GC_DT_SECS * decay_scale).max(0.0);
        if gc.remaining_secs > 0.0 {
            continue;
        }

        state.notify_spawner_despawn(spawner_gc, entity);
        eprintln!("gc despawned world entity {entity}");
        commands.entity(entity).despawn();
    }
}
