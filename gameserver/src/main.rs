// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use std::collections::HashMap;
use std::net::SocketAddr;
use bevy::window::ExitCondition;
use common::physics::physics_world::*;
use common::net::{
    quic::*,
    message::{MsgType, NetworkID, NetworkIDResource, SpawnCommand, SpawnKind},
};
use common::tick::Ticker;
#[derive(Resource)]
struct BindAddr(SocketAddr);
use common::master_plugin::MasterPlugin;
use common::pawn::biped;
use common::pawn::pawn::BipedPawnComponent;
use common::weapon::{rifle, weapon::WeaponStats};
use common::debug_println;

fn parse_addr() -> SocketAddr {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--port" {
            if let Some(port) = args.next().and_then(|p| p.parse::<u16>().ok()) {
                return format!("0.0.0.0:{port}").parse().unwrap();
            }
        }
    }
    common::config::SERVER_BIND_ADDRESS.parse().unwrap()
}

fn main() {
    let bind_addr = parse_addr();
    println!("binding to {bind_addr}");
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins.set(LogPlugin { level: Level::ERROR, ..default() })
            .set(WindowPlugin{
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
    );

    app.add_plugins(MasterPlugin);
    app.insert_resource(BindAddr(bind_addr));
    app.init_resource::<PlayerRegistry>();
    app.init_resource::<WeaponRegistry>();

    app.add_systems(Startup, (start_server, spawn_initial_weapons))
        // Ordering: on_message (apply inputs) → step_physics → broadcast_tick (snapshot after step).
        // step_physics is registered by PhysicsPlugin inside MasterPlugin.
        .add_systems(FixedUpdate, on_message.before(step_physics))
        .add_systems(FixedUpdate, broadcast_tick.after(step_physics));

    println!("starting server...\n");
    app.run();
}

/// Maps each connected client to their spawned pawn entity and network id.
#[derive(Resource, Default)]
struct PlayerRegistry(HashMap<ConnectionId, (Entity, NetworkID)>);

/// Tracks weapons.
/// `free`: net_id → entity (weapons lying in the world with a physics body).
/// `held`: net_id → (entity, carrier_entity) (weapons currently carried, no physics body).
#[derive(Resource, Default)]
struct WeaponRegistry {
    free: HashMap<NetworkID, Entity>,
    held: HashMap<NetworkID, (Entity, Entity)>, // weapon_net_id → (weapon_entity, carrier_entity)
}

fn start_server(mut quic: ResMut<QuicManager>, mut server: ResMut<QuinnetServer>, addr: Res<BindAddr>) {
    quic.start_server(&mut server, addr.0);
}

fn spawn_initial_weapons(
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut weapon_registry: ResMut<WeaponRegistry>,
) {
    let net_id = NetworkID(net_ids.get_next_id());
    let transform = Transform::from_translation(Vec3::new(3.0, 2.0, 0.0));
    let entity = rifle::spawn(transform, &mut commands, &mut world);
    commands.entity(entity).insert(net_id.clone());
    weapon_registry.free.insert(net_id, entity);
}

fn on_message(
    mut quic: ResMut<QuicManager>,
    mut registry: ResMut<PlayerRegistry>,
    mut weapon_registry: ResMut<WeaponRegistry>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
    networked: Query<(Entity, &NetworkID)>,
    weapon_stats: Query<&WeaponStats>,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::Connected => {
                let net_id = NetworkID(net_ids.get_next_id());
                let pos = Vec3::new(0.0, 5.0, 0.0);
                let entity = biped::spawn(Transform::from_translation(pos), &mut commands, &mut world);
                commands.entity(entity).insert(net_id.clone());

                // Tell the new client about all existing pawns (as ghosts).
                for (_, (existing_entity, existing_net_id)) in registry.0.iter() {
                    let existing_pos = if let Some(&h) = world.entity_to_handle.get(existing_entity) {
                        if let Some(rb) = world.rigid_body_set.get(h) {
                            let t = rb.position().translation;
                            Vec3::new(t.x, t.y, t.z)
                        } else { Vec3::ZERO }
                    } else { Vec3::ZERO };
                    quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::SpawnCommand(SpawnCommand {
                        net_id: existing_net_id.clone(),
                        position: existing_pos.into(),
                        starting_velocity: Vec3::ZERO.into(),
                        rotation: Quat::IDENTITY.into(),
                        server_tick: tick.tick,
                        kind: SpawnKind::Biped(false),
                    }));
                }

                // Tell the new client about all free weapons.
                for (weapon_net_id, &weapon_entity) in weapon_registry.free.iter() {
                    let weapon_pos = if let Some(&h) = world.entity_to_handle.get(&weapon_entity) {
                        if let Some(rb) = world.rigid_body_set.get(h) {
                            let t = rb.position().translation;
                            Vec3::new(t.x, t.y, t.z)
                        } else { Vec3::ZERO }
                    } else { Vec3::ZERO };
                    quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::SpawnCommand(SpawnCommand {
                        net_id: weapon_net_id.clone(),
                        position: weapon_pos.into(),
                        starting_velocity: Vec3::ZERO.into(),
                        rotation: Quat::IDENTITY.into(),
                        server_tick: tick.tick,
                        kind: SpawnKind::Rifle,
                    }));
                }

                // Tell all existing clients about the new pawn (as a ghost).
                for (&conn_id, _) in registry.0.iter() {
                    quic.send(SendTarget::One(conn_id), Channel::Ordered, &MsgType::SpawnCommand(SpawnCommand {
                        net_id: net_id.clone(),
                        position: pos.into(),
                        starting_velocity: Vec3::ZERO.into(),
                        rotation: Quat::IDENTITY.into(),
                        server_tick: tick.tick,
                        kind: SpawnKind::Biped(false),
                    }));
                }

                // Tell the new client about its own pawn (owned).
                quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::SpawnCommand(SpawnCommand {
                    net_id: net_id.clone(),
                    position: pos.into(),
                    starting_velocity: Vec3::ZERO.into(),
                    rotation: Quat::IDENTITY.into(),
                    server_tick: tick.tick,
                    kind: SpawnKind::Biped(true),
                }));

                registry.0.insert(msg.conn_id, (entity, net_id));
            }
            MsgType::Disconnected => {
                if let Some((entity, net_id)) = registry.0.remove(&msg.conn_id) {
                    debug_println!("GameServer: Player disconnected: entity={} conn={:?}", entity, msg.conn_id);

                    // Despawn any weapon this player was holding.
                    let held: Vec<NetworkID> = weapon_registry.held.iter()
                        .filter(|&(_, &(_, carrier))| carrier == entity)
                        .map(|(wid, _)| wid.clone())
                        .collect();
                    for wid in held {
                        if let Some((weapon_entity, _)) = weapon_registry.held.remove(&wid) {
                            commands.entity(weapon_entity).despawn();
                            // Clients already despawned the weapon on WeaponPickup; no DespawnCommand needed.
                        }
                    }

                    world.remove_body(entity);
                    commands.entity(entity).despawn();
                    quic.send(SendTarget::All, Channel::Ordered, &MsgType::DespawnCommand(net_id));
                }
            }
            MsgType::Input(pawn_input) => {
                if let Some(&(entity, _)) = registry.0.get(&msg.conn_id) {
                    let handle_opt = world.entity_to_handle.get(&entity).copied();
                    if let Some(handle) = handle_opt {
                        biped::apply_biped_movement(&mut world, &PhysicsBodyHandle(handle), pawn_input.input, &mut BipedPawnComponent);
                    }
                }
            }
            MsgType::Interact(target_net_id) => {
                let Some(&(player_entity, ref player_net_id)) = registry.0.get(&msg.conn_id) else { continue };
                let player_net_id = player_net_id.clone();
                let weapon_entity = match weapon_registry.free.get(&target_net_id) {
                    Some(&e) => e,
                    None => continue,
                };

                // Range check via physics bodies.
                let player_pos = world.entity_to_handle.get(&player_entity)
                    .and_then(|&h| world.rigid_body_set.get(h))
                    .map(|rb| rb.position().translation);
                let weapon_pos = world.entity_to_handle.get(&weapon_entity)
                    .and_then(|&h| world.rigid_body_set.get(h))
                    .map(|rb| rb.position().translation);

                let in_range = match (player_pos, weapon_pos) {
                    (Some(pp), Some(wp)) => {
                        let d = pp - wp;
                        (d.x * d.x + d.y * d.y + d.z * d.z).sqrt() < 2.0
                    }
                    _ => false,
                };

                if in_range {
                    weapon_registry.free.remove(&target_net_id);
                    weapon_registry.held.insert(target_net_id.clone(), (weapon_entity, player_entity));
                    world.remove_body(weapon_entity);
                    quic.send(SendTarget::All, Channel::Ordered,
                        &MsgType::WeaponPickup(target_net_id, player_net_id));
                }
            }
            MsgType::Fire(weapon_net_id, _tick, origin, direction) => {
                let Some(&(shooter_entity, _)) = registry.0.get(&msg.conn_id) else { continue };

                // Validate that this player actually holds the weapon.
                let weapon_entity = match weapon_registry.held.get(&weapon_net_id) {
                    Some(&(we, carrier)) if carrier == shooter_entity => we,
                    _ => continue,
                };

                let origin_v: Vec3 = origin.into();
                let dir_v: Vec3 = direction.into();
                // Normalize direction defensively.
                let dir_v = dir_v.normalize_or_zero();
                if dir_v == Vec3::ZERO { continue; }

                let range = weapon_stats.get(weapon_entity)
                    .map(|s| s.range)
                    .unwrap_or(500.0);
                let hit = world.cast_ray(origin_v, dir_v, range, Some(shooter_entity));

                let (end, hit_net_id) = match hit {
                    Some((hit_entity, toi)) => {
                        let hit_net_id = networked.iter()
                            .find(|(e, _)| *e == hit_entity)
                            .map(|(_, nid)| nid.clone());
                        (origin_v + dir_v * toi, hit_net_id)
                    }
                    None => (origin_v + dir_v * range, None),
                };

                quic.send(SendTarget::All, Channel::Unreliable,
                    &MsgType::HitResult(origin, end.into(), hit_net_id));
            }
            MsgType::Ping(text) => {
                debug_println!("Got a ping from conn_id {:?} with text {}", msg.conn_id, text);
                quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::Pong(text));
            }
            MsgType::ChatMessage(sender, text) => debug_println!("GameServer: Got ChatMessage: [{sender}] {text}"),
            other => debug_println!("Unhandled: {other:?}"),
        }
    }
}

fn broadcast_tick(
    mut quic: ResMut<QuicManager>,
    tick: Res<Ticker>,
    world: Res<PhysicsWorld>,
    query: Query<(&NetworkID, &PhysicsBodyHandle)>,
) {
    let state = snapshot_bodies(&world, tick.tick, query.iter());
    quic.send(SendTarget::All, Channel::Unreliable, &MsgType::State(state));
}
