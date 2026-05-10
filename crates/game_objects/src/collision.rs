use bevy::prelude::*;
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
    pub position: Vec3,
    pub is_new: bool,
}

#[derive(Resource, Default)]
/// Per-tick collision impact buffer consumed by health, sound, and other gameplay systems.
pub struct CollisionImpacts(pub Vec<CollisionImpact>);

/// Registers the shared collision-impact collection pass.
pub struct CollisionPlugin;
impl Plugin for CollisionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CollisionImpacts>().add_systems(
            FixedUpdate,
            collect_collision_impacts
                .after(step_physics)
                .in_set(CollisionImpactSet),
        );
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

        if let Some(entity) = entity1 {
            impacts.0.push(CollisionImpact {
                entity,
                other: entity2,
                impulse,
                position,
                is_new,
            });
        }
        if let Some(entity) = entity2 {
            impacts.0.push(CollisionImpact {
                entity,
                other: entity1,
                impulse,
                position,
                is_new,
            });
        }
    }
}
