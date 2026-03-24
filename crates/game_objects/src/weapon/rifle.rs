use bevy::prelude::*;
use crate::{GameObjectKind, GameObject};
use crate::sound::SoundRequest;
use common::interaction::Interactable;
use physics::physics_world::*;
use crate::generic::hull_or;
use rapier3d::prelude::*;
use super::{Weapon, WeaponComponent, FireCtx};
use crate::projectile::rifle;

const HULL_PATH: &str = "collision/placeholder_ar.obj";

pub const COOLDOWN_TICKS: u32 = 6; // 10 rounds/sec at 60 Hz

pub struct RiflePlugin;
impl Plugin for RiflePlugin {
    fn build(&self, _app: &mut App) {}
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

        let sv = ctx.shooter
            .and_then(|e| world.entity_to_handle.get(&e).copied())
            .and_then(|h| world.rigid_body_set.get(h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);
        let velocity = ctx.aim_dir * rifle::SPEED + sv;
        let temp_id = ctx.id_counter.as_mut().map(|c| { **c = c.wrapping_add(1); **c }).unwrap_or(0);
        rifle::spawn(ctx.origin, velocity, commands, world, ctx.shooter, temp_id);

        if let Some(sq) = ctx.sound.as_mut() {
            // local player: 2D event (no spatialization); remote: 3D at their position
            if ctx.camera.is_some() {
                sq.0.push(SoundRequest { event: "event:/Weapons/RifleShotLocal", position: None, velocity: Vec3::ZERO });
            } else {
                sq.0.push(SoundRequest { event: "event:/Weapons/RifleShot", position: Some(ctx.origin), velocity: Vec3::ZERO });
            }
        }
        if let Some(cam) = ctx.camera.as_mut() { cam.add_kick((2.0, 0.5), (-1.0, 1.0), 20.0); }
        if let (Some(q), Some(id)) = (ctx.quic.as_mut(), ctx.net_id) {
            q.send(net::quic::SendTarget::All, net::quic::Channel::Unordered,
                &net::message::MsgType::FireRequest { weapon: id.clone(), kind: net::message::GameObjectKind::RifleProjectile, temp_id, origin: ctx.origin, dir: ctx.aim_dir });
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
            physics.insert_body(entity, rb)
        };
        world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
        let col = hull_or(HULL_PATH, ColliderBuilder::cuboid(0.2, 0.05, 0.4), world);
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
        collider_set.insert_with_parent(col, rb_handle, rigid_body_set);
        #[cfg(feature = "client")]
        {
            let scene = world.resource::<AssetServer>().load("models/placeholder_ar.glb#Scene0");
            world.entity_mut(entity).insert((SceneRoot(scene), Visibility::default()));
        }
    }
}

