// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use std::collections::HashMap;
use game_common::physics::physics_world::*;
use game_common::net::{
    quic::*,
    runtime::TokioRuntime,
    message::{MsgType, SimulationState, NetworkID, NetworkIDResource, SpawnCommand},
};
use game_common::tick::{increment_tick, Ticker};
use game_common::config::SERVER_BIND_ADDRESS;
use game_common::master_plugin::MasterPlugin;
use game_common::debug_println;
// use game_common::pawn::pawn::BipedPawnComponent;

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins.set(LogPlugin { level: Level::ERROR, ..default() })
    );

    app.add_plugins(MasterPlugin);
    app.insert_resource(NetworkIDResource::default());
    app.init_resource::<PlayerRegistry>();

    app.add_systems(Startup, start_server)
        .add_systems(Update, on_message)
        .add_systems(FixedUpdate, (on_message, increment_tick, broadcast_tick).chain());

    println!("starting server...\n");
    app.run();
}

/// Maps each connected client to their spawned pawn entity.
#[derive(Resource, Default)]
struct PlayerRegistry(HashMap<ConnectionId, Entity>);

fn start_server(mut quic: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    quic.start_server(&runtime, SERVER_BIND_ADDRESS.parse().unwrap());
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
                // let net_id = NetworkID(net_ids.get_next_id());
                // let pos = Vec3::new(0.0, 5.0, 0.0);
                // let entity = spawn_pawn_server(pos, net_id.clone(), &mut commands, &mut world);
                // registry.0.insert(msg.conn_id, entity);
                // quic.send(
                //     SendTarget::One(msg.conn_id),
                //     Channel::Ordered,
                //     &MsgType::SpawnCommand(SpawnCommand {
                //         net_id,
                //         position: pos.into(),
                //         starting_velocity: Vec3::ZERO.into(),
                //         rotation: Quat::IDENTITY.into(),
                //     }),
                // );
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

// dude this is terrible, keep meshes on server, keep it simple, use the existing spawn function
// /// Spawns a physics-only pawn entity on the server (no mesh).
// fn spawn_pawn_server(
//     pos: Vec3,
//     net_id: NetworkID,
//     commands: &mut Commands,
//     world: &mut PhysicsWorld,
// ) -> Entity {
//     use rapier3d::prelude::*;
//     let entity = commands.spawn((
//         BipedPawnComponent,
//         net_id,
//         Transform::from_translation(pos),
//     )).id();
//     let rb = RigidBodyBuilder::dynamic().translation(pos.into()).build();
//     let rb_handle = world.insert_body(entity, rb);
//     let collider = ColliderBuilder::ball(0.5).build();
//     commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
//     let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
//     collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
//     entity
// }
