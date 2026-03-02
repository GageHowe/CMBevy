// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use std::collections::HashMap;
use bevy::window::ExitCondition;
use game_common::physics::physics_world::*;
use game_common::net::{
    quic::*,
    message::{MsgType, SimulationState, NetworkID, NetworkIDResource, SpawnCommand},
};
use game_common::tick::{increment_tick, Ticker};
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
        // on_message only in FixedUpdate; process_inbound (PreUpdate) fills the queue beforehand.
        // increment_tick is registered by MasterPlugin in FixedPostUpdate — don't duplicate it.
        .add_systems(FixedUpdate, (on_message, broadcast_tick).chain());

    println!("starting server...\n");
    app.run();
}

/// Maps each connected client to their spawned pawn entity.
#[derive(Resource, Default)]
struct PlayerRegistry(HashMap<ConnectionId, Entity>);

fn start_server(mut quic: ResMut<QuicManager>, mut server: ResMut<QuinnetServer>) {
    quic.start_server(&mut server, SERVER_BIND_ADDRESS.parse().unwrap());
}

fn on_message(
    mut quic: ResMut<QuicManager>,
    mut registry: ResMut<PlayerRegistry>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::Connected => {
                let net_id = NetworkID(net_ids.get_next_id());
                let pos = Vec3::new(0.0, 5.0, 0.0);
                let entity = biped::spawn_server(Transform::from_translation(pos), &mut commands, &mut world);
                commands.entity(entity).insert(net_id.clone());
                registry.0.insert(msg.conn_id, entity);
                quic.send(
                    SendTarget::One(msg.conn_id),
                    Channel::Ordered,
                    &MsgType::SpawnCommand(SpawnCommand {
                        net_id,
                        position: pos.into(),
                        starting_velocity: Vec3::ZERO.into(),
                        rotation: Quat::IDENTITY.into(),
                    }),
                );
            }
            MsgType::Disconnected => {
                if let Some(entity) = registry.0.remove(&msg.conn_id) {
                    debug_println!("GameServer: Player with entity ID {} and conn_id {:?} disconnected!", entity, &msg.conn_id);
                    world.remove_body(entity);
                    commands.entity(entity).despawn();
                }
            }
            MsgType::Ping(text) => {
                debug_println!("Got a ping from conn_id {:?} with text {}", &msg.conn_id, text);
                quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::Pong(text));
            }
            MsgType::ChatMessage(sender, text) => debug_println!("GameServer: Got ChatMessage: [{sender}] {text}"),
            other => debug_println!("Unhandled: {other:?}"),
        }
    }
}

fn broadcast_tick(mut quic: ResMut<QuicManager>, tick: Res<Ticker>) {
    let msg = MsgType::State(SimulationState { tick: tick.tick, bodies: Default::default() });
    quic.send(SendTarget::All, Channel::Unreliable, &msg);
}

