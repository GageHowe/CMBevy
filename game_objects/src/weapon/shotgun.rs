use bevy::prelude::*;
use crate::{GameObjectKind, GameObject};
use common::interaction::Interactable;
use physics::physics_world::*;
use rapier3d::prelude::*;
use super::{Weapon, WeaponComponent, FireCtx};

pub const RANGE: f32 = 25.0;
/// Total damage split evenly across all pellets on a direct hit.
pub const DAMAGE: f32 = 80.0;
/// Ticks between shots (1 shot/sec at 60 Hz).
pub const COOLDOWN_TICKS: u32 = 60;
/// Number of pellets fired per shot (client-side spread prediction only).
pub const PELLETS: usize = 8;
/// Half-angle spread in radians per pellet offset.
pub const SPREAD: f32 = 0.08;

/// Per-instance state for the shotgun weapon type.
#[derive(Component, Default, Reflect)]
pub struct ShotgunComponent {
    pub cooldown: u32,
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
}

impl Weapon for ShotgunComponent {
    fn fixed_update(&mut self, _world: &mut PhysicsWorld, _commands: &mut Commands, ctx: &mut FireCtx) {
        if ctx.want_fire { self.fire_requested = true; }
        self.cooldown = self.cooldown.saturating_sub(1);
        if !self.fire_requested || self.cooldown > 0 { return; }
        self.cooldown = COOLDOWN_TICKS;
        self.fire_requested = false;
        if let (Some(q), Some(id)) = (ctx.quic.as_mut(), ctx.net_id) {
            q.send(net::quic::SendTarget::All, net::quic::Channel::Unordered,
                   &net::message::MsgType::Fire(id.clone(), ctx.origin.into(), ctx.aim_dir.into(), ctx.tick));
        }
    }
}

impl GameObject for ShotgunComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform { translation: cmd.position, rotation: cmd.rotation, scale: Vec3::ONE };
        world.entity_mut(entity).insert((
            WeaponComponent,
            ShotgunComponent::default(),
            GameObjectKind::Shotgun,
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
            let scene = world.resource::<AssetServer>().load("models/shotgun.glb#Scene0");
            world.entity_mut(entity).insert((SceneRoot(scene), Visibility::default()));
        }
    }
}
