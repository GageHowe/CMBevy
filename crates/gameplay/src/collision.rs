use bevy::prelude::*;
#[cfg(feature = "client")]
use particles_plugin::prelude::{spawn_dust_impact_effect, spawn_sparks_impact_effect};
use physics::physics_world::{PhysicsWorld, step_physics};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
/// Orders collision-impact collection after the Rapier step.
pub struct CollisionImpactSet;

#[derive(Clone, Copy)]
/// One gameplay-facing collision observation derived from the Rapier contact graph.
pub struct CollisionImpact {
    pub entity: Entity,
    pub other: Option<Entity>,
    pub impulse: f32,
    pub relative_speed: f32,
    pub position: Vec3,
    pub is_new: bool,
}

#[derive(Resource, Default)]
/// Per-tick collision impact buffer consumed by health, sound, and other gameplay systems.
pub struct CollisionImpacts(pub Vec<CollisionImpact>);

#[derive(Component, Clone, Copy, Reflect, PartialEq, Eq, Debug)]
#[reflect(Component)]
pub enum CollisionFxMaterial {
    Dust,
    Sparks,
}

impl CollisionFxMaterial {
    #[cfg(feature = "client")]
    fn min_relative_speed(self) -> f32 {
        match self {
            Self::Dust => 8.0,
            Self::Sparks => 12.0,
        }
    }
}

/// Registers the shared collision-impact collection pass.
pub struct CollisionPlugin;
impl Plugin for CollisionPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<CollisionFxMaterial>()
            .init_resource::<CollisionImpacts>()
            .add_systems(
                FixedUpdate,
                collect_collision_impacts
                    .after(step_physics)
                    .in_set(CollisionImpactSet),
            );
        #[cfg(feature = "client")]
        app.add_systems(FixedUpdate, spawn_collision_fx.after(CollisionImpactSet));
    }
}

fn collect_collision_impacts(world: Res<PhysicsWorld>, mut impacts: ResMut<CollisionImpacts>) {
    // clear last tick's impact events
    impacts.0.clear();
    for pair in world.narrow_phase.contact_pairs() {
        let Some(collider1) = world.collider_set.get(pair.collider1) else {
            continue;
        };
        let Some(collider2) = world.collider_set.get(pair.collider2) else {
            continue;
        };
        let impulse = pair.total_impulse_magnitude();
        let mut position = None;
        let mut is_new = false;
        for manifold in &pair.manifolds {
            is_new |= manifold
                .data
                .solver_contacts
                .iter()
                .any(|contact| contact.is_new > 0.5);
            for contact in &manifold.points {
                position.get_or_insert_with(|| {
                    let point1 = collider1.position().rotation * contact.local_p1
                        + collider1.position().translation;
                    let point2 = collider2.position().rotation * contact.local_p2
                        + collider2.position().translation;
                    (point1 + point2) * 0.5
                });
            }
        }
        if impulse <= 0.0 {
            continue;
        }

        let entity1 = world
            .collider_set
            .get(pair.collider1)
            .and_then(|collider| collider.parent())
            .and_then(|handle| world.handle_to_entity.get(&handle).copied());
        let entity2 = world
            .collider_set
            .get(pair.collider2)
            .and_then(|collider| collider.parent())
            .and_then(|handle| world.handle_to_entity.get(&handle).copied());
        let position = position.unwrap_or(Vec3::ZERO);
        let relative_speed = match (entity1, entity2) {
            (Some(entity1), Some(entity2)) => (collision_fx_velocity(&world, entity1)
                - collision_fx_velocity(&world, entity2))
            .length(),
            (Some(entity1), None) => collision_fx_velocity(&world, entity1).length(),
            (None, Some(entity2)) => collision_fx_velocity(&world, entity2).length(),
            (None, None) => 0.0,
        };

        if let Some(entity) = entity1 {
            impacts.0.push(CollisionImpact {
                entity,
                other: entity2,
                impulse,
                relative_speed,
                position,
                is_new,
            });
        }
        if let Some(entity) = entity2 {
            impacts.0.push(CollisionImpact {
                entity,
                other: entity1,
                impulse,
                relative_speed,
                position,
                is_new,
            });
        }
    }
}

#[cfg(feature = "client")]
fn spawn_collision_fx(
    impacts: Res<CollisionImpacts>,
    materials: Query<&CollisionFxMaterial>,
    world: Res<PhysicsWorld>,
    mut commands: Commands,
) {
    for impact in &impacts.0 {
        if !impact.is_new {
            continue;
        }
        let Ok(&material) = materials.get(impact.entity) else {
            continue;
        };
        if impact.relative_speed < material.min_relative_speed() {
            continue;
        }
        let position = impact.position;
        let inherit_velocity = collision_fx_velocity(&world, impact.entity).clamp_length_max(40.0);
        match material {
            CollisionFxMaterial::Dust => commands.queue(move |world: &mut World| {
                spawn_dust_impact_effect(world, position, inherit_velocity);
            }),
            CollisionFxMaterial::Sparks => commands.queue(move |world: &mut World| {
                spawn_sparks_impact_effect(world, position, inherit_velocity);
            }),
        }
    }
}

fn collision_fx_velocity(world: &PhysicsWorld, entity: Entity) -> Vec3 {
    world
        .entity_to_handle
        .get(&entity)
        .and_then(|&handle| world.rigid_body_set.get(handle))
        .map(physics::physics_world::rb_vel)
        .unwrap_or(Vec3::ZERO)
}
