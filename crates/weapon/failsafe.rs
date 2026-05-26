// use bevy::prelude::*;
// use net::{message::NetworkID, quic::ConnectionId};
// use rapier3d::prelude::ColliderBuilder;
// #[cfg(feature = "client")]
// use physics::physics_world::*;

// use super::*;
// #[cfg(feature = "client")]
// use crate::pawn::CameraShake;
// use crate::{
//     NetworkEntityMap,
//     pawn::{PlayerRegistry, WeaponSlots},
//     projectile,
// };

// pub const COOLDOWN_TICKS: u32 = 45;
// pub const MAGAZINE_SIZE: u16 = 1;
// pub const RESERVE_AMMO: u16 = 5;
// pub const RELOAD_TICKS: u16 = 95;

// #[derive(Component, Default, Reflect)]
// pub struct FailsafeComponent {
//     pub trigger_down: bool,
// }

// pub const CONFIG: WeaponConfig = WeaponConfig {
//         display_name: "Failsafe",
//         model_path: "models/launcher_placeholder_2.glb#Scene0",
//         collider_path: "collision/placeholder_ar.obj",
//         crosshair_path: "textures/crosshairs/crosshair028.png",
//         prediction_projectile_speed: Some(projectile::FAILSAFE_SPEED),
//         zoom_multiplier: 1.0,
//         magazine_size: MAGAZINE_SIZE,
//         reserve_ammo: RESERVE_AMMO,
//         reload_ticks: RELOAD_TICKS,
//         fire_cooldown_ticks: COOLDOWN_TICKS as u16,
//         projectile_kind: net::message::GameObjectKind::FailsafeProjectile,
//         fire_projectile: projectile::fire_authoritative,
// };

// #[cfg(feature = "client")]
// fn update_failsafe(
//     weapon: &mut FailsafeComponent,
//     world: &mut PhysicsWorld,
//     commands: &mut Commands,
//     ctx: &mut FireCtx,
// ) {
//     if weapon.trigger_down && !ctx.want_fire {
//         request_detonate_local(commands, ctx.weapon);
//         #[cfg(feature = "client")]
//         if let (Some(quic), Some(weapon_net_id)) = (ctx.quic.as_deref_mut(), ctx.net_id) {
//             quic.send_to_server(
//                 net::quic::Channel::Ordered,
//                 &net::message::MsgType::DetonateFailsafeRequest(weapon_net_id.clone()),
//             );
//         }
//     }
//     if ctx.reload_pressed {
//         super::start_reload(ctx.weapon_state, &ctx.weapon_config);
//     }
//     if !ctx.want_fire {
//         weapon.trigger_down = false;
//         return;
//     }
//     if weapon.trigger_down || !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
//         return;
//     }
//     weapon.trigger_down = true;
//     let velocity =
//         crate::projectile::projectile_velocity(world, ctx.shooter, ctx.aim_dir, projectile::FAILSAFE_SPEED);
//     let temp_id = crate::projectile::next_temp_id(ctx.id_counter.as_deref_mut());
//     projectile::spawn_failsafe_with_weapon(
//         ctx.origin,
//         velocity,
//         commands,
//         world,
//         ctx.shooter,
//         ctx.weapon,
//         temp_id,
//     );
//     #[cfg(feature = "client")]
//     helpers::send_fire_request(
//         ctx.quic.as_deref_mut(), ctx.net_id, ctx.weapon_config.projectile_kind.clone(), temp_id, ctx.origin, ctx.aim_dir,
//     );
//     #[cfg(feature = "client")]
//     helpers::apply_local_predicted_impulse(
//         ctx, world, -ctx.aim_dir * 3.0 * helpers::shooter_mass(world, ctx.shooter),
//     );
//     helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/SniperShotLocal");
//     if let Some(cam) = ctx.camera.as_mut() {
//         cam.add_kick((8.0, 10.0), (-2.0, 2.0), 8.0);
//         #[cfg(feature = "client")]
//         cam.add_shake(CameraShake {
//             translation: Vec3::new(0.01, 0.01, 0.08),
//             rotation: Vec2::new(0.02, 0.015),
//             roll: 0.01,
//             duration: 0.18,
//             frequency: 16.0,
//         });
//     }
// }

// #[cfg(feature = "client")]
// pub fn drive_failsafes(
//     mut weapons: Query<(Entity, &mut FailsafeComponent, &mut WeaponState, &WeaponConfig, &PendingWeaponInput)>,
//     net_ids: Query<&net::message::NetworkID>,
//     world: ResMut<PhysicsWorld>,
//     commands: Commands,
//     quic: Option<ResMut<net::quic::QuicManager>>,
//     sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
//     possessed: Query<Entity, With<crate::pawn::Possessed>>,
//     camera_fx: Query<(&mut crate::pawn::CameraEffector, &GlobalTransform), With<Camera3d>>,
//     id_counter: Option<ResMut<crate::projectile::ProjectileIdCounter>>,
//     predicted: Option<ResMut<common::PredictedCommands>>,
// ) {
//     super::drive_weapon_inputs(&mut weapons, net_ids, world, commands, quic, sound_queue, possessed, camera_fx, id_counter, predicted, update_failsafe);
// }

// pub fn spawn_failsafe(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
//         let weapon = weapon_bundle(FailsafeComponent::default(), CONFIG);
//         helpers::insert_generic_weapon(
//             entity,
//             cmd,
//             world,
//             CONFIG.display_name,
//             CONFIG.model_path,
//             CONFIG.crosshair_path,
//             CONFIG.prediction_projectile_speed,
//             weapon,
//         );
//         helpers::make_generic_weapon_physics(
//             entity,
//             cmd,
//             CONFIG.collider_path,
//             ColliderBuilder::cuboid(0.2, 0.06, 0.55),
//             world,
//         );
//         crate::insert_spawn_metadata(entity, world, Some(10.0), true, None, true);
// }

// pub fn request_detonate_local(commands: &mut Commands, weapon_entity: Entity) {
//     commands.queue(move |world: &mut World| {
//         projectile::detonate_latest_failsafe_for_weapon(world, weapon_entity);
//     });
// }

// pub fn handle_detonate_failsafe_request(
//     conn_id: ConnectionId,
//     weapon_net_id: NetworkID,
//     registry: &PlayerRegistry,
//     all_networked: &NetworkEntityMap,
//     pawn_slots: &Query<&mut WeaponSlots>,
//     commands: &mut Commands,
// ) {
//     let Some((shooter_entity, _)) = registry.character(conn_id) else {
//         return;
//     };
//     let shooter_holds = pawn_slots
//         .get(shooter_entity)
//         .map(|s| s.contains_net_id(&weapon_net_id))
//         .unwrap_or(false);
//     if !shooter_holds {
//         return;
//     }
//     let Some(weapon_entity) = all_networked.get(&weapon_net_id) else {
//         return;
//     };
//     request_detonate_local(commands, weapon_entity);
// }
