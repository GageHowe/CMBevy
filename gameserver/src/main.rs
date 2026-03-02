// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use std::collections::HashMap;
use bevy::window::ExitCondition;
use game_common::physics::physics_world::*;
use game_common::net::{
    quic::*,
    message::{MsgType, NetworkID, NetworkIDResource, SpawnCommand},
};
use game_common::tick::Ticker;
use game_common::config::SERVER_BIND_ADDRESS;
use game_common::master_plugin::MasterPlugin;
use game_common::pawn::biped;
use game_common::debug_println;

fn main() {
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
    app.init_resource::<PlayerRegistry>();

    app.add_systems(Startup, start_server)
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

fn start_server(mut quic: ResMut<QuicManager>, mut server: ResMut<QuinnetServer>) {
    quic.start_server(&mut server, SERVER_BIND_ADDRESS.parse().unwrap());
}

fn on_message(
    mut quic: ResMut<QuicManager>,
    mut registry: ResMut<PlayerRegistry>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::Connected => {
                let net_id = NetworkID(net_ids.get_next_id());
                let pos = Vec3::new(0.0, 5.0, 0.0);
                let entity = biped::spawn_server(Transform::from_translation(pos), &mut commands, &mut world);
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
                        is_owned: false,
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
                        is_owned: false,
                    }));
                }

                // Tell the new client about its own pawn (owned).
                quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::SpawnCommand(SpawnCommand {
                    net_id: net_id.clone(),
                    position: pos.into(),
                    starting_velocity: Vec3::ZERO.into(),
                    rotation: Quat::IDENTITY.into(),
                    server_tick: tick.tick,
                    is_owned: true,
                }));

                registry.0.insert(msg.conn_id, (entity, net_id));
            }
            MsgType::Disconnected => {
                if let Some((entity, net_id)) = registry.0.remove(&msg.conn_id) {
                    debug_println!("GameServer: Player disconnected: entity={} conn={:?}", entity, msg.conn_id);
                    world.remove_body(entity);
                    commands.entity(entity).despawn();
                    // Tell remaining clients to remove the ghost.
                    quic.send(SendTarget::All, Channel::Ordered, &MsgType::DespawnCommand(net_id));
                }
            }
            MsgType::Input(pawn_input) => {
                if let Some(&(entity, _)) = registry.0.get(&msg.conn_id) {
                    let handle_opt = world.entity_to_handle.get(&entity).copied();
                    if let Some(handle) = handle_opt {
                        biped::apply_biped_movement(&mut world, &PhysicsBodyHandle(handle), pawn_input.input);
                    }
                }
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

