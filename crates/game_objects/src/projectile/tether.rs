use super::Projectile;
use crate::health::Health;
use crate::weapon::tether::{self, TetherSide};
use bevy::prelude::*;
use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::*;

#[derive(Component, Default, Reflect)]
pub struct TetherHookProjectile;

impl Projectile for TetherHookProjectile {
    const KIND: GameObjectKind = GameObjectKind::TetherHookProjectile;
    const SPEED: f32 = 0.0;

    fn tick(
        &mut self,
        _entity: Entity,
        _body: &RigidBodyHandleComponent,
        _world: &mut PhysicsWorld,
        _commands: &mut Commands,
        _health_q: &mut Query<&mut Health, Without<Shield>>,
    ) {
    }

    fn fire_authoritative(
        origin: Vec3,
        dir: Vec3,
        shooter: Entity,
        _tick: u64,
        weapon: Entity,
        temp_id: u32,
        commands: &mut Commands,
        _world: &mut PhysicsWorld,
        _net_ids: &mut net::message::NetworkIDResource,
    ) -> Option<super::FiredProjectile> {
        commands.queue(move |world: &mut World| {
            world.resource_scope(|world, mut physics: Mut<PhysicsWorld>| {
                let Some(mut gun) = world.get_mut::<tether::TetherGunComponent>(weapon) else {
                    return;
                };
                tether::fire_contact(
                    &mut gun,
                    TetherSide::from_temp_id(temp_id),
                    Some(shooter),
                    origin,
                    dir,
                    &mut physics,
                );
            });
        });
        None
    }

    fn spawn_predicted(
        origin: Vec3,
        _velocity: Vec3,
        commands: &mut Commands,
        _world: &mut PhysicsWorld,
        _shooter: Option<Entity>,
        _weapon: Option<Entity>,
        _temp_id: u32,
    ) -> Entity {
        commands.spawn(Transform::from_translation(origin)).id()
    }
}

pub fn spawn_remote_tether_hook(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
    world.entity_mut(entity).insert((
        GameObjectKind::TetherHookProjectile,
        TetherHookProjectile,
        Transform::from_translation(cmd.position),
    ));
}

pub struct TetherHookProjectilePlugin;
impl Plugin for TetherHookProjectilePlugin {
    fn build(&self, _app: &mut App) {}
}
