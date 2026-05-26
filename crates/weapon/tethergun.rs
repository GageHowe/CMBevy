// use super::{FireCtx, Weapon, helpers};
// use crate::{GameObject, GameObjectKind};
// use bevy::prelude::*;
// use physics::physics_world::*;
// use rapier3d::prelude::{ColliderBuilder, ImpulseJointHandle};

// const COOLDOWN_TICKS: u32 = 12;
// pub const HOOK_RANGE: f32 = 180.0;

// pub struct TetherGunPlugin;
// impl Plugin for TetherGunPlugin {
//     fn build(&self, app: &mut App) {
//         app.add_systems(FixedUpdate, clear_invalid_tethers.before(step_physics));
//         #[cfg(feature = "client")]
//         app.add_systems(Update, draw_tether_lines);
//     }
// }

// #[derive(Clone, Copy, PartialEq, Eq, Reflect)]
// pub enum TetherSide {
//     Left,
//     Right,
// }

// impl TetherSide {
//     pub fn bit(self) -> u32 {
//         match self {
//             Self::Left => 0,
//             Self::Right => 1,
//         }
//     }

//     pub fn from_temp_id(temp_id: u32) -> Self {
//         if temp_id & 1 == 1 {
//             Self::Right
//         } else {
//             Self::Left
//         }
//     }

//     pub fn from_bit(bit: u8) -> Self {
//         if bit == 1 { Self::Right } else { Self::Left }
//     }
// }

// // Unfinished experimental weapon; keep unspawned from maps until tether sync is rebuilt.
// #[derive(Clone, Copy, Reflect)]
// pub struct TetherEndpoint {
//     pub entity: Entity,
//     pub local_anchor: Vec3,
// }

// #[derive(Component, Default, Reflect)]
// pub struct TetherGunComponent {
//     pub cooldown: u32,
//     pub left: Option<TetherEndpoint>,
//     pub right: Option<TetherEndpoint>,
//     pub fallback: Option<TetherEndpoint>,
//     #[reflect(ignore)]
//     pub joint: Option<ImpulseJointHandle>,
//     #[reflect(ignore)]
//     body_enabled: bool,
// }

// impl TetherGunComponent {
//     fn endpoint(&self, side: TetherSide) -> Option<TetherEndpoint> {
//         match side {
//             TetherSide::Left => self.left,
//             TetherSide::Right => self.right,
//         }
//     }

//     fn endpoint_mut(&mut self, side: TetherSide) -> &mut Option<TetherEndpoint> {
//         match side {
//             TetherSide::Left => &mut self.left,
//             TetherSide::Right => &mut self.right,
//         }
//     }

//     fn other_endpoint(&self, side: TetherSide) -> Option<TetherEndpoint> {
//         match side {
//             TetherSide::Left => self.right,
//             TetherSide::Right => self.left,
//         }
//     }

//     fn clear(&mut self, world: &mut PhysicsWorld) {
//         remove_joint(self, world);
//         self.left = None;
//         self.right = None;
//         self.fallback = None;
//     }
// }

// impl Weapon for TetherGunComponent {
//     const MODEL_PATH: &'static str = "models/tether_placeholder.glb#Scene0";
//     const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
//     const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair013.png";
//     const PREDICTION_PROJECTILE_SPEED: Option<f32> = None;

//     fn fixed_update(
//         &mut self,
//         world: &mut PhysicsWorld,
//         _commands: &mut Commands,
//         ctx: &mut FireCtx,
//     ) {
//         self.cooldown = self.cooldown.saturating_sub(1);
//         let side = if ctx.want_fire {
//             Some(TetherSide::Left)
//         } else if ctx.want_alt_fire {
//             Some(TetherSide::Right)
//         } else {
//             None
//         };
//         let Some(side) = side else { return };
//         if self.cooldown > 0 {
//             return;
//         }
//         self.cooldown = COOLDOWN_TICKS;

//         #[cfg(feature = "client")]
//         if let Some(quic) = ctx.quic.as_deref_mut()
//             && quic.client_connected
//             && let Some(net_id) = ctx.net_id
//         {
//             quic.send(
//                 net::quic::SendTarget::All,
//                 net::quic::Channel::Ordered,
//                 &net::message::MsgType::FireRequest {
//                     weapon: net_id.clone(),
//                     kind: GameObjectKind::TetherHookProjectile,
//                     temp_id: side.bit(),
//                     origin: ctx.origin,
//                     dir: ctx.aim_dir,
//                 },
//             );
//             return;
//         }

//         fire_contact(self, side, ctx.shooter, ctx.origin, ctx.aim_dir, world);
//     }
// }

// impl GameObject for TetherGunComponent {
//     fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
//         helpers::insert_generic_weapon(
//             entity,
//             cmd,
//             world,
//             GameObjectKind::TetherGun,
//             <Self as Weapon>::MODEL_PATH,
//             <Self as Weapon>::CROSSHAIR_PATH,
//             <Self as Weapon>::PREDICTION_PROJECTILE_SPEED,
//             TetherGunComponent::default(),
//         );
//         helpers::make_generic_weapon_physics(
//             entity,
//             cmd,
//             <Self as Weapon>::COLLIDER_PATH,
//             ColliderBuilder::cuboid(0.18, 0.06, 0.32),
//             world,
//         );
//     }
// }

// pub fn fire_contact(
//     weapon: &mut TetherGunComponent,
//     side: TetherSide,
//     shooter: Option<Entity>,
//     origin: Vec3,
//     dir: Vec3,
//     world: &mut PhysicsWorld,
// ) {
//     if weapon.endpoint(side).is_some() {
//         clear_side(weapon, side, world);
//         return;
//     }
//     if weapon.left.is_some() && weapon.right.is_some() {
//         weapon.clear(world);
//     }
//     remove_joint(weapon, world);
//     if weapon.left.is_some() || weapon.right.is_some() {
//         weapon.fallback = None;
//     }
//     let exclude = shooter.map(|entity| [entity]).unwrap_or([Entity::PLACEHOLDER]);
//     let Some((hit, toi)) = world.cast_ray(origin, dir, HOOK_RANGE, &exclude) else {
//         return;
//     };
//     attach_endpoint(
//         weapon,
//         side,
//         shooter,
//         hit,
//         origin + dir.normalize_or_zero() * toi,
//         origin,
//         world,
//     );
// }

// pub fn clear_side(weapon: &mut TetherGunComponent, side: TetherSide, world: &mut PhysicsWorld) {
//     remove_joint(weapon, world);
//     *weapon.endpoint_mut(side) = None;
//     if weapon.left.is_none() && weapon.right.is_none() {
//         weapon.fallback = None;
//     }
//     rebuild_joint(weapon, world);
// }

// pub fn sync_endpoints(
//     weapon: &mut TetherGunComponent,
//     left: Option<TetherEndpoint>,
//     right: Option<TetherEndpoint>,
//     fallback: Option<TetherEndpoint>,
// ) {
//     weapon.left = left;
//     weapon.right = right;
//     weapon.fallback = fallback;
// }

// fn attach_endpoint(
//     weapon: &mut TetherGunComponent,
//     side: TetherSide,
//     shooter: Option<Entity>,
//     target: Entity,
//     hit_point: Vec3,
//     player_anchor: Vec3,
//     world: &mut PhysicsWorld,
// ) {
//     let Some(hit_endpoint) = endpoint_from_world(world, target, hit_point) else {
//         return;
//     };
//     *weapon.endpoint_mut(side) = Some(hit_endpoint);
//     if weapon.other_endpoint(side).is_none()
//         && let Some(shooter) = shooter
//         && let Some(player_endpoint) = endpoint_from_world(world, shooter, player_anchor)
//     {
//         weapon.fallback = Some(player_endpoint);
//     } else if weapon.left.is_some() && weapon.right.is_some() {
//         weapon.fallback = None;
//     }
//     rebuild_joint(weapon, world);
// }

// fn remove_joint(weapon: &mut TetherGunComponent, world: &mut PhysicsWorld) {
//     if let Some(joint) = weapon.joint.take() {
//         world.remove_impulse_joint(joint);
//     }
// }

// fn endpoint_from_world(
//     world: &PhysicsWorld,
//     entity: Entity,
//     world_anchor: Vec3,
// ) -> Option<TetherEndpoint> {
//     let rb = world
//         .entity_to_handle
//         .get(&entity)
//         .and_then(|&h| world.rigid_body_set.get(h))?;
//     Some(TetherEndpoint {
//         entity,
//         local_anchor: rb_rot(rb).inverse() * (world_anchor - rb_pos(rb)),
//     })
// }

// fn endpoint_world(world: &PhysicsWorld, endpoint: TetherEndpoint) -> Option<Vec3> {
//     let rb = world
//         .entity_to_handle
//         .get(&endpoint.entity)
//         .and_then(|&h| world.rigid_body_set.get(h))?;
//     Some(rb_pos(rb) + rb_rot(rb) * endpoint.local_anchor)
// }

// fn endpoint_enabled(world: &PhysicsWorld, endpoint: TetherEndpoint) -> bool {
//     world
//         .entity_to_handle
//         .get(&endpoint.entity)
//         .and_then(|&h| world.rigid_body_set.get(h))
//         .is_some_and(|rb| rb.is_enabled())
// }

// fn active_endpoints(weapon: &TetherGunComponent) -> Option<(TetherEndpoint, TetherEndpoint)> {
//     match (weapon.left, weapon.right, weapon.fallback) {
//         (Some(left), Some(right), _) => Some((left, right)),
//         (Some(left), None, Some(fallback)) => Some((left, fallback)),
//         (None, Some(right), Some(fallback)) => Some((right, fallback)),
//         _ => None,
//     }
// }

// fn rebuild_joint(weapon: &mut TetherGunComponent, world: &mut PhysicsWorld) {
//     let Some((left, right)) = active_endpoints(weapon) else {
//         return;
//     };
//     if left.entity == right.entity {
//         weapon.clear(world);
//         return;
//     }
//     let (Some(left_world), Some(right_world)) =
//         (endpoint_world(world, left), endpoint_world(world, right))
//     else {
//         return;
//     };
//     weapon.joint = world.insert_rope_joint(
//         left.entity,
//         right.entity,
//         left.local_anchor,
//         right.local_anchor,
//         left_world.distance(right_world).max(0.25),
//         true,
//     );
// }

// fn clear_invalid_tethers(
//     mut world: ResMut<PhysicsWorld>,
//     mut weapons: Query<(&mut TetherGunComponent, &RigidBodyHandleComponent)>,
// ) {
//     for (mut weapon, handle) in &mut weapons {
//         let enabled = world
//             .rigid_body_set
//             .get(handle.0)
//             .is_some_and(|rb| rb.is_enabled());
//         let picked_up = weapon.body_enabled && !enabled;
//         weapon.body_enabled = enabled;
//         let invalid = active_endpoints(&weapon).is_some_and(|(left, right)| {
//             !endpoint_enabled(&world, left) || !endpoint_enabled(&world, right)
//         });
//         if picked_up || invalid {
//             weapon.clear(&mut world);
//         }
//     }
// }

// #[cfg(feature = "client")]
// fn draw_tether_lines(
//     world: Res<PhysicsWorld>,
//     weapons: Query<&TetherGunComponent>,
//     mut gizmos: Gizmos,
// ) {
//     for weapon in &weapons {
//         let Some((left, right)) = active_endpoints(weapon) else {
//             continue;
//         };
//         let (Some(left), Some(right)) =
//             (endpoint_world(&world, left), endpoint_world(&world, right))
//         else {
//             continue;
//         };
//         gizmos.line(left, right, Color::srgb(0.7, 0.9, 1.0));
//     }
// }
