use bevy::prelude::*;
use physics::physics_world::*;
use crate::health::Health;
use crate::GameObject;
pub use common::GameObjectKind;

pub mod rifle;
pub mod hail_mary;

pub struct ProjectilePlugin;
impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "client")]
        app.init_resource::<ProjectileIdCounter>();
        app.add_plugins((rifle::RifleProjectilePlugin, hail_mary::HailMaryProjectilePlugin));
    }
}


/// Per-projectile-type behavior. Analogous to Weapon / Pawn.
/// The implementing type IS the component (fields: shooter, lifetime, etc.).
/// Requires GameObject so spawn-from-SpawnCommand is also defined per type.
pub trait Projectile: Component<Mutability = bevy::ecs::component::Mutable> + GameObject {
    const KIND: GameObjectKind;
    /// Handles lifetime, hit detection, and on-hit effects.
    /// Server-side / singleplayer only — caller registers with appropriate run_if.
    fn tick(&mut self, entity: Entity, body: &RigidBodyHandleComponent,
            world: &PhysicsWorld, commands: &mut Commands, health_q: &mut Query<&mut Health>);
}

/// Thin shared component — only temp_id is cross-cutting (ProjectileConfirm matching on client).
#[derive(Component, Default, Reflect)]
pub struct ProjectileState {
    pub temp_id: u32,
}

/// Client resource: monotonically increasing counter for temp projectile ids.
#[derive(Resource, Default)]
pub struct ProjectileIdCounter {
    pub count: u32,
}

/// Returns a system that draws all projectiles of type P using gizmos.
/// Color is captured at registration time. Not all projectile types need to use this.
#[cfg(feature = "client")]
pub fn draw_projectile_debug<P: Component>(color: Color) -> impl Fn(Res<PhysicsWorld>, Query<&RigidBodyHandleComponent, With<P>>, Gizmos) {
    move |world, projectiles, mut gizmos| {
        use physics::debug::{draw_collider, rb_iso};
        for body_handle in projectiles.iter() {
            let Some(rb) = world.rigid_body_set.get(body_handle.0) else { continue };
            let iso = rb_iso(rb);
            for ch in rb.colliders() {
                if let Some(col) = world.collider_set.get(*ch) {
                    draw_collider(col, iso, color, &mut gizmos);
                }
            }
        }
    }
}

/// Generic tick system. Analogous to move_pawns<T>.
/// Server-only; each projectile plugin registers with run_if(in_state(GameState::SinglePlayer)) on client.
pub fn tick_projectiles<P: Projectile>(
    world: Res<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut P, &RigidBodyHandleComponent)>,
    mut health_q: Query<&mut Health>,
) {
    for (entity, mut proj, body) in q.iter_mut() {
        proj.tick(entity, body, &world, &mut commands, &mut health_q);
    }
}
