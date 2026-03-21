use bevy::prelude::*;
use crate::{GameObjectKind, GameObject};
use crate::health::Health;
use crate::sound::SoundRequest;
use common::interaction::Interactable;
use physics::debug::{draw_collider, rb_iso};
use physics::physics_world::*;
use rapier3d::prelude::*;
use super::{Weapon, WeaponComponent, FireCtx, RemoteFireQueue};

pub const DAMAGE: f32 = 25.0;
pub const COOLDOWN_TICKS: u32 = 6;      // 10 rounds/sec at 60 Hz
pub const PROJECTILE_SPEED: f32 = 600.0;
pub const PROJECTILE_LIFETIME: u32 = 120; // 2 seconds at 60 Hz

pub struct RiflePlugin;
impl Plugin for RiflePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, (
            apply_remote_rifle_fires.before(step_physics),
            tick_rifle_hits.after(step_physics),
        ));
        #[cfg(feature = "client")]
        app.add_systems(Update, add_rifle_projectile_visual);
    }
}

#[derive(Component, Default, Reflect)]
pub struct RifleComponent {
    pub cooldown: u32,
}

impl Weapon for RifleComponent {
    fn fixed_update(&mut self, world: &mut PhysicsWorld, commands: &mut Commands, ctx: &mut FireCtx) {
        self.cooldown = self.cooldown.saturating_sub(1);
        if !ctx.want_fire || self.cooldown > 0 { return; }
        self.cooldown = COOLDOWN_TICKS;
        spawn_rifle_projectile(ctx.origin, ctx.aim_dir, commands, world, ctx.shooter);
        if let Some(sq) = ctx.sound.as_mut() {
            // local player: 2D event (no spatialization); remote: 3D at their position
            if ctx.camera.is_some() {
                sq.0.push(SoundRequest { event: "event:/Weapons/RifleShotLocal", position: None, velocity: Vec3::ZERO });
            } else {
                sq.0.push(SoundRequest { event: "event:/Weapons/RifleShot", position: Some(ctx.origin), velocity: Vec3::ZERO });
            }
        }
        if let Some(cam) = ctx.camera.as_mut() { cam.add_kick((1.5, 1.0), (-0.5, 0.5), 10.0); }
        if let (Some(q), Some(id), Some(sid)) = (ctx.quic.as_mut(), ctx.net_id, ctx.shooter_net_id) {
            q.send(net::quic::SendTarget::All, net::quic::Channel::Unordered,
                   &net::message::MsgType::RifleFire { weapon: id.clone(), shooter: sid.clone(), origin: ctx.origin, dir: ctx.aim_dir, tick: ctx.tick });
        }
    }
}

impl GameObject for RifleComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform { translation: cmd.position, rotation: cmd.rotation, scale: Vec3::ONE };
        world.entity_mut(entity).insert((
            WeaponComponent,
            RifleComponent::default(),
            GameObjectKind::Rifle,
            Interactable { range: 2.0 },
            Transform::from(transform),
            cmd.net_id.clone(),
        ));
        let rb_handle = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let rb = RigidBodyBuilder::dynamic().translation(transform.translation).angular_damping(2.0).build();
            let rb_handle = physics.insert_body(entity, rb);
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
            collider_set.insert_with_parent(ColliderBuilder::cuboid(0.2, 0.05, 0.4).build(), rb_handle, rigid_body_set);
            rb_handle
        };
        world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
        #[cfg(feature = "client")]
        {
            let scene = world.resource::<AssetServer>().load("models/ar.glb#Scene0");
            world.entity_mut(entity).insert((SceneRoot(scene), Visibility::default()));
        }
    }
}

#[derive(Component, Default, Reflect)]
pub struct RifleProjectileState {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}

pub fn spawn_rifle_projectile(
    origin: Vec3,
    direction: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    shooter: Option<Entity>,
) {
    let sv = shooter
        .and_then(|e| world.entity_to_handle.get(&e).copied())
        .and_then(|h| world.rigid_body_set.get(h))
        .map(|rb| { let v = rb.linvel(); Vec3::new(v.x, v.y, v.z) })
        .unwrap_or(Vec3::ZERO);
    let vel = direction * PROJECTILE_SPEED + sv;
    let entity = commands.spawn((
        RifleProjectileState { shooter, lifetime: PROJECTILE_LIFETIME },
        Transform::from_translation(origin),
        Visibility::default(),
    )).id();
    let rb_handle = world.insert_body(entity, RigidBodyBuilder::kinematic_velocity_based()
        .translation(origin)
        .linvel(Vector::new(vel.x, vel.y, vel.z))
        .ccd_enabled(true)
        .build());
    {
        let proj_collision = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL, InteractionTestMode::And);
        let proj_solver    = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL & !GROUP_PLAYER, InteractionTestMode::And);
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
        collider_set.insert_with_parent(
            ColliderBuilder::ball(0.03).collision_groups(proj_collision).solver_groups(proj_solver).build(),
            rb_handle, rigid_body_set,
        );
    }
    commands.entity(entity).insert(RigidBodyHandleComponent(rb_handle));
}

/// Drains remote rifle fire events (from other clients) and calls fixed_update to spawn projectiles + play sound.
fn apply_remote_rifle_fires(
    mut queue: ResMut<RemoteFireQueue>,
    mut weapons: Query<(&net::message::NetworkID, &mut RifleComponent)>,
    networked: Query<(Entity, &net::message::NetworkID)>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut sound: Option<ResMut<crate::sound::SoundQueue>>,
) {
    for (weapon_nid, shooter_nid, origin, dir, tick) in queue.rifle.drain(..) {
        let Some((_, mut comp)) = weapons.iter_mut().find(|(nid, _)| **nid == weapon_nid) else { continue };
        let shooter = networked.iter().find(|(_, nid)| **nid == shooter_nid).map(|(e, _)| e);
        let mut ctx = FireCtx {
            want_fire: true, want_alt_fire: false,
            origin, aim_dir: dir,
            shooter, tick,
            net_id: None, shooter_net_id: None,
            sound: sound.as_deref_mut(),
            camera: None, quic: None,
        };
        comp.fixed_update(&mut world, &mut commands, &mut ctx);
    }
}

fn tick_rifle_hits(
    world: Res<PhysicsWorld>,
    mut commands: Commands,
    mut projectiles: Query<(Entity, &mut RifleProjectileState, &RigidBodyHandleComponent)>,
    mut health_q: Query<&mut Health>,
) {
    let mut hits: Vec<(Entity, Entity)> = Vec::new();
    for (proj_entity, mut state, rb_handle) in projectiles.iter_mut() {
        state.lifetime = state.lifetime.saturating_sub(1);
        if state.lifetime == 0 { commands.entity(proj_entity).despawn(); continue; }
        let Some(rb) = world.rigid_body_set.get(rb_handle.0) else { continue };
        'outer: for &ch in rb.colliders() {
            for pair in world.narrow_phase.contact_pairs_with(ch) {
                if !pair.has_any_active_contact() { continue; }
                let other_ch = if pair.collider1 == ch { pair.collider2 } else { pair.collider1 };
                let Some(other_rb) = world.collider_set.get(other_ch).and_then(|c| c.parent()) else { continue };
                let Some(&hit_entity) = world.handle_to_entity.get(&other_rb) else { continue };
                if state.shooter == Some(hit_entity) { continue; }
                hits.push((proj_entity, hit_entity));
                break 'outer;
            }
        }
    }
    for (proj, target) in hits {
        commands.entity(proj).despawn();
        if let Ok(mut health) = health_q.get_mut(target) { health.apply_damage(DAMAGE); }
    }
}

pub fn draw_projectile_debug(
    world: Res<PhysicsWorld>,
    projectiles: Query<(&RifleProjectileState, &RigidBodyHandleComponent)>,
    mut gizmos: Gizmos,
) {
    for (_, body_handle) in projectiles.iter() {
        let Some(rb) = world.rigid_body_set.get(body_handle.0) else { continue };
        let iso = rb_iso(rb);
        for ch in rb.colliders() {
            if let Some(col) = world.collider_set.get(*ch) {
                draw_collider(col, iso, Color::srgba(1.0, 0.9, 0.2, 0.9), &mut gizmos);
            }
        }
    }
}

#[cfg(feature = "client")]
fn add_rifle_projectile_visual(
    q: Query<Entity, Added<RifleProjectileState>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(0.04));
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.2),
            emissive: LinearRgba::new(6.0, 5.0, 0.5, 1.0),
            unlit: true,
            ..default()
        });
        commands.entity(entity).insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
