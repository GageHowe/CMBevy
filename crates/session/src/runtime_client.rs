use bevy::{core_pipeline::Skybox, prelude::*, state::state::FreelyMutableState};
use common::GameObjectKind;
use game_objects::{
    Team,
    bot::{BotContext, BotController},
    health::Health,
    level::{
        LevelSceneRoot, MapMeta, PendingMapScene, SpawnPoint, compressed_level_hash,
        default_asset_dir, load_level_source, read_cached_map, write_cached_map,
    },
    lifecycle::{pick_spawn_point_with_velocity, spawn_game_object},
    pawn::{InteractionGate, Possessed, biped::BipedPawnComponent, spaceship::SpaceshipPawnComponent},
};
use net::{
    message::{MsgType, NetworkIDResource},
    quic::QuicManager,
};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};

use crate::{
    hosted::{cleanup_before_app_exit, exit_after_returning_to_menu, shutdown_session},
    messages,
    resources::*,
};

pub struct ClientSessionPlugin<S: States + FreelyMutableState + Copy> {
    pub main_menu: S,
    pub single_player: S,
    pub multiplayer: S,
}

impl<S: States + FreelyMutableState + Copy> Plugin for ClientSessionPlugin<S> {
    fn build(&self, app: &mut App) {
        let main_menu = self.main_menu;
        let single_player = self.single_player;
        let multiplayer = self.multiplayer;
        app.insert_resource(GuiState::default())
            .configure_sets(
                FixedUpdate,
                game_objects::health::HealthAuthoritySet.run_if(crate::runtime::has_authority),
            )
            .configure_sets(
                FixedUpdate,
                game_objects::projectile::ProjectileAuthoritySet
                    .run_if(crate::runtime::has_authority),
            )
            .configure_sets(
                FixedUpdate,
                game_objects::level::LevelAuthoritySet.run_if(crate::runtime::has_authority),
            )
            .configure_sets(
                common::slow_update::SlowUpdate,
                game_objects::level::LevelAuthoritySet.run_if(crate::runtime::has_authority),
            )
            .init_resource::<LastServerState>()
            .init_resource::<LastAckedInputSeq>()
            .init_resource::<PendingWorldReady>()
            .init_resource::<PendingReconciliation>()
            .init_resource::<PendingWeaponPickups>()
            .insert_resource(ClientSessionState { main_menu })
            .add_systems(
                OnEnter(single_player),
                (reset_singleplayer_spawn_state, load_sp_level::<S>).chain(),
            )
            .add_systems(
                OnExit(single_player),
                (reset_interaction_gate, cleanup_world, remove_script).chain(),
            )
            .add_systems(FixedUpdate, respawn_singleplayer.run_if(in_state(single_player)))
            .add_systems(FixedUpdate, run_singleplayer_bots.run_if(in_state(single_player)))
            .add_systems(OnEnter(multiplayer), (reset_interaction_gate, connect).chain())
            .add_systems(OnExit(multiplayer), (cleanup_world, disconnect, remove_script).chain())
            .add_systems(Update, show_transport_notices.run_if(in_state(multiplayer)))
            .add_systems(Update, messages::retry_weapon_pickups.run_if(in_state(multiplayer)))
            .add_systems(Update, send_world_ready.run_if(in_state(multiplayer)))
            .add_systems(Update, mark_world_ready_after_level_load.run_if(in_state(multiplayer)))
            .add_systems(Update, load_skybox.run_if(resource_added::<MapMeta>))
            .add_systems(Last, cleanup_before_app_exit)
            .add_systems(Update, exit_after_returning_to_menu.run_if(in_state(main_menu)))
            .add_systems(FixedPostUpdate, messages::on_message::<S>)
            .add_systems(
                FixedLast,
                crate::runtime::snapshot_server_state
                    .run_if(in_state(multiplayer).and(resource_changed::<PendingReconciliation>)),
            );
    }
}

fn show_transport_notices(mut quic: Option<ResMut<QuicManager>>, mut commands: Commands) {
    let Some(quic) = quic.as_mut() else {
        return;
    };
    while let Some(message) = quic.notices.pop_front() {
        game_objects::messages::push(&mut commands, message);
    }
}

#[derive(Resource, Clone, Copy)]
pub(crate) struct ClientSessionState<S: States + Copy> {
    pub main_menu: S,
}

fn reset_singleplayer_spawn_state(mut sp: ResMut<SinglePlayerConfig>) {
    sp.timer = None;
    sp.spawned_once = false;
}

fn reset_interaction_gate(mut interaction: ResMut<InteractionGate>) {
    *interaction = InteractionGate::default();
}

fn load_sp_level<S: States + FreelyMutableState + Copy>(
    mut commands: Commands,
    sp: Res<SinglePlayerConfig>,
) {
    let _ = std::marker::PhantomData::<S>;
    if sp.map.is_empty() || sp.gametype.is_empty() {
        game_objects::messages::push(&mut commands, "No map or mode selected.");
        return;
    }
    commands.insert_resource(scripting::ScriptConfig {
        path: sp.gametype.clone(),
        is_server: true,
        source: None,
    });
    game_objects::messages::push(&mut commands, "Loading map...");
    match load_level_source(&sp.map, &default_asset_dir()) {
        Ok(level) => commands.insert_resource(PendingMapScene(level.compressed)),
        Err(err) => game_objects::messages::push(&mut commands, format!("Map load failed: {err}")),
    };
}

fn respawn_singleplayer(
    time: Res<Time>,
    possessed: Query<(), With<Possessed>>,
    pending_map: Option<Res<PendingMapScene>>,
    mut commands: Commands,
    mut net_ids: ResMut<NetworkIDResource>,
    spawn_points: Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: Query<&Transform>,
    parent_parents: Query<&ChildOf>,
    parent_bodies: Query<&RigidBodyHandleComponent>,
    physics: Res<PhysicsWorld>,
    mut sp: ResMut<SinglePlayerConfig>,
) {
    if !possessed.is_empty() || pending_map.is_some() {
        sp.timer = None;
        return;
    }
    let respawn_delay = if sp.spawned_once { common::config::RESPAWN_DELAY_SECS } else { 0.0 };
    let remaining = sp.timer.get_or_insert(respawn_delay);
    *remaining -= time.delta_secs();
    if *remaining > 0.0 {
        return;
    }
    sp.timer = None;
    let Some((position, rotation, velocity)) = pick_spawn_point_with_velocity(
        &spawn_points,
        &parent_transforms,
        &parent_parents,
        &parent_bodies,
        &physics,
        0,
        0,
    ) else {
        return;
    };
    let (entity, _, _) = spawn_game_object(
        GameObjectKind::Biped,
        position,
        rotation,
        velocity,
        0,
        &mut commands,
        &mut net_ids,
    );
    commands.entity(entity).insert((Possessed::new(128), Team(0)));
    sp.spawned_once = true;
}

fn run_singleplayer_bots(
    mut bots: Query<(Entity, &mut BotController)>,
    actors: Query<(Entity, &Team, &Health)>,
    mut bipeds: Query<&mut BipedPawnComponent>,
    mut spaceships: Query<&mut SpaceshipPawnComponent>,
    mut world: ResMut<PhysicsWorld>,
) {
    let actors = actors
        .iter()
        .filter_map(|(entity, team, health)| {
            let body = world
                .entity_to_handle
                .get(&entity)
                .and_then(|handle| world.rigid_body_set.get(*handle))?;
            Some(BotContext {
                entity,
                team: *team,
                pos: physics::physics_world::rb_pos(body),
                rot: physics::physics_world::rb_rot(body),
                vel: physics::physics_world::rb_vel(body),
                health: health.current,
                visible: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    for (entity, mut bot) in &mut bots {
        let Some(mut ctx) = actors.iter().find(|actor| actor.entity == entity).cloned() else {
            continue;
        };
        ctx.visible = actors.clone();
        let output = bot.brain.think(&ctx);
        let Some(handle) = world.entity_to_handle.get(&entity).copied() else {
            continue;
        };
        match output.input {
            common::PawnInputKind::Biped(input) => {
                let Ok(mut biped) = bipeds.get_mut(entity) else {
                    continue;
                };
                game_objects::pawn::biped::apply_biped_input(
                    &mut world,
                    entity,
                    input,
                    &RigidBodyHandleComponent(handle),
                    &mut biped,
                );
            }
            common::PawnInputKind::Spaceship(input) => {
                let Ok(mut ship) = spaceships.get_mut(entity) else {
                    continue;
                };
                game_objects::pawn::spaceship::apply_spaceship_movement(
                    &mut world,
                    &RigidBodyHandleComponent(handle),
                    input,
                    &mut ship,
                );
            }
        }
    }
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
    let image: Handle<Image> =
        asset_server.load(game_objects::asset_path::resolve_asset_path(path));
    commands.entity(cam).insert((
        Skybox { image: image.clone(), brightness: meta.skybox_brightness, ..default() },
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
    mut commands: Commands,
    mut quic: ResMut<QuicManager>,
    addr: Res<ServerAddr>,
    mut pending: ResMut<PendingWorldReady>,
) {
    pending.0 = false;
    game_objects::messages::push(&mut commands, "Connecting...");
    quic.connect(addr.0);
}

fn send_world_ready(
    mut quic: ResMut<QuicManager>,
    mut pending: ResMut<PendingWorldReady>,
    pending_map: Option<Res<PendingMapScene>>,
) {
    if !pending.0 || pending_map.is_some() {
        return;
    }
    quic.send(
        net::quic::SendTarget::One(net::quic::SERVER_CONN_ID),
        net::quic::Channel::Ordered,
        &MsgType::ClientReady,
    );
    info!("Client: sent ClientReady");
    pending.0 = false;
}

fn mark_world_ready_after_level_load(
    loaded_levels: Query<(), Added<LevelSceneRoot>>,
    mut pending: ResMut<PendingWorldReady>,
) {
    if !loaded_levels.is_empty() {
        pending.0 = true;
    }
}

fn request_map(quic: &mut QuicManager) {
    quic.send(
        net::quic::SendTarget::One(net::quic::SERVER_CONN_ID),
        net::quic::Channel::Ordered,
        &MsgType::RequestMap,
    );
}

fn disconnect(
    mut quic: ResMut<QuicManager>,
    mut pending: ResMut<PendingReconciliation>,
    mut last_acked: ResMut<LastAckedInputSeq>,
    mut pending_world_ready: ResMut<PendingWorldReady>,
    mut hosted: ResMut<HostedServer>,
) {
    last_acked.0 = 0;
    pending_world_ready.0 = false;
    shutdown_session(Some(&mut quic), Some(&mut pending), &mut hosted);
}

fn remove_script(mut commands: Commands) {
    commands.remove_resource::<scripting::ScriptConfig>();
}

pub(crate) fn handle_map_hash(hash: String, quic: &mut QuicManager, commands: &mut Commands) {
    if let Some(compressed) = read_cached_map(&hash) {
        if compressed_level_hash(&compressed).as_deref() == Some(hash.as_str()) {
            game_objects::messages::push(commands, "Using cached map.");
            commands.insert_resource(PendingMapScene(compressed));
            return;
        }
        game_objects::messages::push(commands, "Cached map invalid. Redownloading.");
    }
    game_objects::messages::push(commands, "Downloading map...");
    request_map(quic);
}

pub(crate) fn handle_file_data(name: String, compressed: Vec<u8>, commands: &mut Commands) {
    if name == "map.scn.ron" {
        let Some(hash) = compressed_level_hash(&compressed) else {
            eprintln!("FileData: failed to hash map.scn.ron");
            return;
        };
        info!("Client: received map.scn.ron {hash}");
        write_cached_map(&hash, &compressed);
        commands.insert_resource(PendingMapScene(compressed));
        return;
    }
    if name != "gametype.lua" {
        return;
    }
    match zstd::stream::decode_all(compressed.as_slice()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(src) => commands.insert_resource(scripting::ScriptConfig {
                path: String::new(),
                is_server: false,
                source: Some(src),
            }),
            Err(e) => eprintln!("FileData: gametype.lua not valid utf8: {e}"),
        },
        Err(e) => eprintln!("FileData: failed to decompress gametype.lua: {e}"),
    };
}
