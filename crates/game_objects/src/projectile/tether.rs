use super::{
    Projectile, ProjectileState, insert_generic_remote_projectile, make_generic_projectile_physics,
};
use crate::GameObject;
use crate::health::Health;
use crate::weapon::tether::{TetherGunComponent, TetherSide, attach_endpoint};
use bevy::prelude::*;
use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::*;
use rapier3d::prelude::Group;

pub const SPEED: f32 = 160.0;
const LIFETIME: u32 = 180;
const RADIUS: f32 = 0.06;

#[derive(Component, Reflect)]
pub struct TetherHookProjectile {
    pub shooter: Option<Entity>,
    pub weapon: Option<Entity>,
    pub side: TetherSide,
    pub player_anchor: Vec3,
    pub lifetime: u32,
}

impl Default for TetherHookProjectile {
    fn default() -> Self {
        Self {
            shooter: None,
            weapon: None,
            side: TetherSide::Left,
            player_anchor: Vec3::ZERO,
            lifetime: LIFETIME,
        }
    }
}

impl Projectile for TetherHookProjectile {
    const KIND: GameObjectKind = GameObjectKind::TetherHookProjectile;
    const SPEED: f32 = SPEED;

    fn tick(
        &mut self,
        _entity: Entity,
        _body: &RigidBodyHandleComponent,
        _world: &mut PhysicsWorld,
        _commands: &mut Commands,
        _health_q: &mut Query<&mut Health>,
    ) {
    }

    fn spawn_predicted(
        origin: Vec3,
        velocity: Vec3,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        shooter: Option<Entity>,
        weapon: Option<Entity>,
        temp_id: u32,
    ) -> Entity {
        spawn(
            origin,
            velocity,
            commands,
            world,
            shooter,
            weapon,
            TetherSide::from_temp_id(temp_id),
            temp_id,
        )
    }
}

pub fn spawn(
    origin: Vec3,
    velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    shooter: Option<Entity>,
    weapon: Option<Entity>,
    side: TetherSide,
    temp_id: u32,
) -> Entity {
    let entity = commands
        .spawn((
            GameObjectKind::TetherHookProjectile,
            TetherHookProjectile {
                shooter,
                weapon,
                side,
                player_anchor: origin,
                lifetime: LIFETIME,
            },
            ProjectileState { temp_id },
            Transform::from_translation(origin),
        ))
        .id();
    let rb_handle =
        make_generic_projectile_physics(entity, origin, velocity, RADIUS, Group::NONE, world);
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    entity
}

impl GameObject for TetherHookProjectile {
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        insert_generic_remote_projectile(
            entity,
            cmd,
            world,
            TetherHookProjectile {
                shooter: None,
                weapon: None,
                side: TetherSide::Left,
                player_anchor: cmd.position,
                lifetime: LIFETIME,
            },
            RADIUS,
            "event:/Weapons/SniperShot",
        );
    }
}

pub struct TetherHookProjectilePlugin;
impl Plugin for TetherHookProjectilePlugin {
    fn build(&self, app: &mut App) {
        use common::game_state::GameState;
        app.add_systems(
            FixedUpdate,
            tick_tether_hooks
                .after(step_physics)
                .run_if(|state: Option<Res<State<GameState>>>| {
                    state.map_or(true, |s| *s.get() == GameState::SinglePlayer)
                }),
        );
        #[cfg(feature = "client")]
        app.add_systems(
            FixedUpdate,
            tick_predicted_tether_hooks
                .after(step_physics)
                .run_if(in_state(GameState::Multiplayer)),
        );
        #[cfg(feature = "client")]
        app.add_systems(Update, add_visual);
        #[cfg(feature = "client")]
        app.add_systems(Update, draw_flying_tethers);
    }
}

fn tick_tether_hooks(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut hooks: Query<(Entity, &mut TetherHookProjectile, &RigidBodyHandleComponent)>,
    mut weapons: Query<&mut TetherGunComponent>,
) {
    for (entity, mut hook, body) in &mut hooks {
        hook.lifetime = hook.lifetime.saturating_sub(1);
        if hook.lifetime == 0 {
            commands.entity(entity).despawn();
            continue;
        }
        let Some(rb) = world.rigid_body_set.get(body.0) else {
            continue;
        };
        let vel = rb_vel(rb);
        let step = vel.length() * world.integration_parameters.dt;
        if step < 0.001 {
            continue;
        }
        let dir = vel.normalize();
        let curr = rb_pos(rb);
        let prev = curr - vel * world.integration_parameters.dt;
        let exclude = [entity, hook.shooter.unwrap_or(entity)];
        let Some((hit, toi)) = world.cast_ray(prev, dir, step, &exclude) else {
            continue;
        };
        if let Some(weapon) = hook.weapon
            && let Ok(mut weapon) = weapons.get_mut(weapon)
        {
            attach_endpoint(
                &mut weapon,
                hook.side,
                hook.shooter,
                hit,
                prev + dir * toi,
                hook.player_anchor,
                &mut world,
                true,
            );
        }
        commands.entity(entity).despawn();
    }
}

#[cfg(feature = "client")]
fn tick_predicted_tether_hooks(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut hooks: Query<(
        Entity,
        &mut TetherHookProjectile,
        &RigidBodyHandleComponent,
        &ProjectileState,
    )>,
    mut weapons: Query<&mut TetherGunComponent>,
) {
    for (entity, mut hook, body, state) in &mut hooks {
        if state.temp_id == 0 {
            continue;
        }
        hook.lifetime = hook.lifetime.saturating_sub(1);
        if hook.lifetime == 0 {
            commands.entity(entity).despawn();
            continue;
        }
        let Some(rb) = world.rigid_body_set.get(body.0) else {
            continue;
        };
        let vel = rb_vel(rb);
        let step = vel.length() * world.integration_parameters.dt;
        if step < 0.001 {
            continue;
        }
        let dir = vel.normalize();
        let curr = rb_pos(rb);
        let prev = curr - vel * world.integration_parameters.dt;
        let exclude = [entity, hook.shooter.unwrap_or(entity)];
        let Some((hit, toi)) = world.cast_ray(prev, dir, step, &exclude) else {
            continue;
        };
        if let Some(weapon) = hook.weapon
            && let Ok(mut weapon) = weapons.get_mut(weapon)
        {
            attach_endpoint(
                &mut weapon,
                hook.side,
                hook.shooter,
                hit,
                prev + dir * toi,
                hook.player_anchor,
                &mut world,
                true,
            );
        }
        commands.entity(entity).despawn();
    }
}

#[cfg(feature = "client")]
fn add_visual(
    q: Query<Entity, Added<TetherHookProjectile>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(RADIUS));
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.8, 1.0),
            emissive: LinearRgba::new(1.5, 4.0, 8.0, 1.0),
            unlit: true,
            ..default()
        });
        commands
            .entity(entity)
            .insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}

#[cfg(feature = "client")]
fn draw_flying_tethers(
    world: Res<PhysicsWorld>,
    hooks: Query<(&TetherHookProjectile, &RigidBodyHandleComponent)>,
    mut gizmos: Gizmos,
) {
    for (hook, body) in &hooks {
        let Some(rb) = world.rigid_body_set.get(body.0) else {
            continue;
        };
        gizmos.line(hook.player_anchor, rb_pos(rb), Color::srgb(0.7, 0.9, 1.0));
    }
}
