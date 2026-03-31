use bevy::core_pipeline::Skybox;
use bevy::prelude::*;
use common::GameObjectKind;
use game_objects::SpawnGameObjectCommand;
use game_objects::level::{LevelSceneRoot, MapMeta};
use game_objects::pawn::Possessed;
use net::message::{NetworkID, NetworkIDResource, SimulationState, SpawnCommand};
use net::quic::{QuicManager, QuinnetClient};

use crate::reconciliation::PendingReconciliation;
use crate::settings::Settings;
use crate::{GameState, HostedServer, ServerAddr, SinglePlayerConfig};

pub struct ClientSessionPlugin;
impl Plugin for ClientSessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastServerState>()
            .add_systems(
                OnEnter(GameState::SinglePlayer),
                (load_sp_level, spawn_local_player).chain(),
            )
            .add_systems(
                OnExit(GameState::SinglePlayer),
                (cleanup_world, remove_script).chain(),
            )
            .add_systems(OnEnter(GameState::Multiplayer), connect)
            .add_systems(
                OnExit(GameState::Multiplayer),
                (cleanup_world, disconnect, remove_script).chain(),
            )
            .add_systems(Update, load_skybox.run_if(resource_added::<MapMeta>))
            .add_systems(
                Update,
                draw_server_state
                    .run_if(debug_render_on)
                    .run_if(in_state(GameState::Multiplayer)),
            );
    }
}

#[derive(Resource, Default)]
pub struct LastServerState(pub Option<SimulationState>);

fn load_sp_level(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sp: Res<SinglePlayerConfig>,
) {
    let map = if sp.map.is_empty() {
        "maps/default.scn.ron".to_string()
    } else {
        sp.map.clone()
    };
    if !sp.gametype.is_empty() {
        commands.insert_resource(scripting::ScriptConfig {
            path: sp.gametype.clone(),
            is_server: false,
            source: None,
        });
    }
    commands.spawn((
        bevy::scene::DynamicSceneRoot(asset_server.load(map)),
        LevelSceneRoot,
    ));
}

fn spawn_local_player(mut commands: Commands, mut net_ids: ResMut<NetworkIDResource>) {
    let cmd = SpawnCommand {
        net_id: NetworkID(net_ids.next()),
        position: Vec3::new(0.0, 800.0, 0.0),
        rotation: Quat::IDENTITY,
        starting_velocity: Vec3::ZERO,
        server_tick: 0,
        kind: GameObjectKind::Biped,
    };
    let entity = commands.spawn_empty().id();
    commands.queue(SpawnGameObjectCommand { entity, cmd });
    commands.entity(entity).insert(Possessed::new(128));
}

fn load_skybox(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    meta: Res<MapMeta>,
    camera: Query<Entity, With<Camera3d>>,
) {
    let (Some(path), Ok(cam)) = (&meta.skybox, camera.single()) else {
        return;
    };
    let image: Handle<Image> = asset_server.load(path.clone());
    commands.entity(cam).insert((
        Skybox {
            image: image.clone(),
            brightness: meta.skybox_brightness,
            ..default()
        },
        EnvironmentMapLight {
            diffuse_map: image.clone(),
            specular_map: image,
            intensity: meta.env_light_intensity,
            affects_lightmapped_mesh_diffuse: true,
            ..default()
        },
    ));
}

pub fn cleanup_world(
    mut commands: Commands,
    camera: Query<(Entity, Option<&Children>), With<Camera3d>>,
    roots: Query<Entity, (With<Transform>, Without<Camera3d>, Without<ChildOf>)>,
) {
    if let Ok((cam, children)) = camera.single() {
        if let Some(ch) = children {
            for child in ch.iter() {
                commands.queue(move |world: &mut World| {
                    if let Ok(entity) = world.get_entity_mut(child) {
                        entity.despawn();
                    }
                });
            }
        }
        commands.queue(move |world: &mut World| {
            if let Ok(mut entity) = world.get_entity_mut(cam) {
                entity.remove_parent_in_place();
            }
        });
    }
    for entity in roots.iter() {
        commands.queue(move |world: &mut World| {
            if let Ok(entity) = world.get_entity_mut(entity) {
                entity.despawn();
            }
        });
    }
}

fn connect(
    mut quic: ResMut<QuicManager>,
    mut client: ResMut<QuinnetClient>,
    addr: Res<ServerAddr>,
) {
    quic.connect(&mut client, addr.0);
}

fn disconnect(
    mut quic: ResMut<QuicManager>,
    mut client: ResMut<QuinnetClient>,
    mut pending: ResMut<PendingReconciliation>,
    mut hosted: ResMut<HostedServer>,
) {
    if let Some(conn) = client.get_connection_mut() {
        let _ = conn.disconnect();
    }
    hosted.stdin = None;
    if let Some(mut child) = hosted.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Some(id) = hosted.beacon_id.lock().unwrap().take() {
        std::thread::spawn(move || {
            let _ = ureq::delete(&format!("{}/lobbies/{id}", common::config::BEACON_URL)).call();
        });
    }
    quic.inbound.clear();
    quic.client_connected = false;
    pending.0 = None;
}

fn remove_script(mut commands: Commands) {
    commands.remove_resource::<scripting::ScriptConfig>();
}

pub fn snapshot_server_state(
    pending: Res<PendingReconciliation>,
    mut last: ResMut<LastServerState>,
) {
    if let Some(st) = &pending.0 {
        last.0 = Some(st.clone());
    }
}

fn draw_server_state(last: Res<LastServerState>, mut gizmos: Gizmos) {
    let Some(state) = &last.0 else { return };
    for body in state.bodies.values() {
        let pos: Vec3 = body.position.into();
        gizmos.sphere(pos, 0.15, Color::srgb(1.0, 0.2, 0.2));
    }
}

fn debug_render_on(settings: Res<Settings>) -> bool {
    settings.debug_render
}
