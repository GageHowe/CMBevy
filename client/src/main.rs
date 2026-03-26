// client executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;
use camera::spawn_camera;
use common::GameObjectKind;
pub use common::game_state::GameState;
use common::interaction::Interactable;
use common::tick::Ticker;
use game_objects::SpawnGameObjectCommand;
use game_objects::pawn::biped::*;
use game_objects::pawn::*;
use game_objects::projectile::rifle::*;
use game_objects::projectile::*;
use game_objects::weapon::WeaponPlugin;
use net::{message::*, quic::*};
use physics::physics_world::*;
use reconciliation::*;
use std::net::SocketAddr;
use tick_sync::{NetworkStats, TickSyncPlugin};
use ui::ui::UIPlugin;
use ui::window::WindowSettingsPlugin;

mod camera;
mod menu;
mod outline;
mod reconciliation;
mod session;
mod tick_sync;
mod ui;
use menu::MenuPlugin;
use outline::OutlinePlugin;
use session::{ClientSessionPlugin, LastServerState};

#[derive(Resource)]
pub(crate) struct ServerAddr(pub SocketAddr);

/// Child process handle when we spawned a local gameserver.
#[derive(Resource, Default)]
pub(crate) struct HostedServer {
    pub child: Option<std::process::Child>,
    pub stdin: Option<std::io::BufWriter<std::process::ChildStdin>>,
    /// Filled by the beacon register thread once registration succeeds; cleared on deregister.
    pub beacon_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl HostedServer {
    pub fn send_command(&mut self, cmd: &str) {
        use std::io::Write;
        if let Some(w) = &mut self.stdin {
            let _ = writeln!(w, "{cmd}");
            let _ = w.flush();
        }
    }
}

/// Bundles spawn-related parameters to stay within Bevy's 16-param SystemParam limit.
#[derive(bevy::ecs::system::SystemParam)]
struct SpawnParams<'w, 's> {
    commands: Commands<'w, 's>,
    entity_children: Query<'w, 's, &'static Children>,
    lights: Query<'w, 's, &'static mut Visibility, With<SpotLight>>,
}
use common::debug_println;
use game_objects::health::Health;
use game_objects::level::{
    LevelPlugin, LevelSceneRoot, MapMeta, PendingMapScene, apply_pending_map_scene, cleanup_level,
    load_level_scene,
};
use game_objects::planet::draw_planet_radii;
use master_plugin::MasterPlugin;
use settings::{Settings, SettingsPlugin};
use steam::SteamworksPlugin;
// use game_objects::pawn::biped::draw_biped_debug; // don't do debug for bipeds for now
use ui::ui::GuiState;
mod settings;
mod sound;
mod steam;
use sound::SoundPlugin;

/// Map and gametype selected in the singleplayer setup screen.
#[derive(Resource, Default)]
pub(crate) struct SinglePlayerConfig {
    pub map: String,
    pub gametype: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
pub(crate) enum UiState {
    #[default]
    Playing,
    Paused,
    Settings,
}

fn parse_server_addr() -> SocketAddr {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--server" {
            if let Some(addr) = args.next().and_then(|a| a.parse().ok()) {
                return addr;
            }
        }
    }
    common::config::SERVER_BIND_ADDRESS.parse().unwrap()
}

fn main() {
    let server_addr = parse_server_addr();
    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: if cfg!(debug_assertions) {
                    "../assets"
                } else {
                    "assets"
                }
                .to_string(),
                ..default()
            })
            .set(LogPlugin {
                level: Level::WARN,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Critical Mass".into(),
                    present_mode: PresentMode::FifoRelaxed,
                    ..default()
                }),
                ..default()
            }),
    );

    app.add_plugins(OutlinePlugin)
        .add_plugins(MasterPlugin)
        .add_plugins(SteamworksPlugin)
        .add_plugins(SettingsPlugin)
        .init_state::<GameState>()
        .init_state::<UiState>()
        .add_plugins(WindowSettingsPlugin)
        .add_plugins(UIPlugin)
        .add_plugins(MenuPlugin)
        .add_plugins(PawnPlugin)
        .add_plugins(LevelPlugin)
        .add_plugins(WeaponPlugin)
        .add_plugins(SoundPlugin)
        .add_plugins(ClientSessionPlugin)
        .add_plugins(ReconciliationPlugin::<GameState>::new(
            GameState::Multiplayer,
        ))
        .add_plugins(TickSyncPlugin(GameState::Multiplayer))
        .insert_resource(ServerAddr(server_addr))
        .init_resource::<SinglePlayerConfig>()
        .init_resource::<HostedServer>()
        // PendingHullColliders now managed by LevelPlugin
        .add_systems(
            FixedUpdate,
            step_physics
                .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))),
        )
        .add_systems(
            Update,
            sync_physics_visual
                .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))),
        )
        .add_systems(Startup, spawn_camera);
    app.add_systems(OnExit(GameState::SinglePlayer), cleanup_level);
    app.add_systems(OnExit(GameState::Multiplayer), cleanup_level);

    // FixedPreUpdate ordering:
    //   maybe_reconcile → GatherInputSet (per-pawn-type gather) → send_X_input → MovePawnsSet
    // (maybe_reconcile registered by ReconciliationPlugin)
    app.add_systems(
        FixedPreUpdate,
        pawn::send_pawn_input
            .after(GatherInputSet)
            .before(MovePawnsSet)
            .run_if(in_state(GameState::Multiplayer)),
    );

    // FixedPostUpdate:
    //   record_world_state (ReconciliationPlugin) → on_message/send_chat
    app.add_systems(
        FixedPostUpdate,
        on_message.run_if(in_state(GameState::Multiplayer)),
    );

    app.add_systems(
        FixedPostUpdate,
        session::snapshot_server_state.after(on_message).run_if(
            in_state(GameState::Multiplayer).and(resource_changed::<PendingReconciliation>),
        ),
    );

    // (tick increment is FixedLast)

    app.add_systems(Update, load_level_scene.run_if(resource_added::<MapMeta>));
    app.add_systems(Update, apply_pending_map_scene);
    app.add_systems(Update, draw_planet_radii.run_if(debug_render_on));
    app.add_systems(
        FixedUpdate,
        (
            draw_projectile_debug::<HailMaryProjectile>(Color::srgba(1.0, 0.3, 0.1, 0.9)),
            draw_projectile_debug::<RifleProjectile>(Color::srgba(1.0, 0.9, 0.2, 0.9)),
        )
            .after(step_physics)
            .run_if(debug_render_on)
            .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))),
    );

    debug_println!("starting client...\n");
    app.run();
}

fn spawn_local_player(mut commands: Commands, mut net_ids: ResMut<NetworkIDResource>) {
    // scene loads async; spawn at the default map origin — scene's SpawnPoint y=800 matches
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

fn cleanup_world(
    mut commands: Commands,
    camera: Query<(Entity, Option<&Children>), With<Camera3d>>,
    roots: Query<Entity, (With<Transform>, Without<Camera3d>, Without<ChildOf>)>,
) {
    if let Ok((cam, children)) = camera.single() {
        // despawn weapon viewmodels (children of camera) before detaching
        if let Some(ch) = children {
            for child in ch.iter() {
                commands.entity(child).despawn();
            }
        }
        commands.entity(cam).remove_parent_in_place();
    }
    for entity in roots.iter() {
        commands.entity(entity).despawn();
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
    hosted.stdin = None; // close stdin first so server gets EOF
    if let Some(mut child) = hosted.child.take() {
        let _ = child.kill();
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

/// handles messages coming in from the server
/// called by quic on FixedPostUpdate
fn on_message(
    mut quic: ResMut<QuicManager>,
    mut gui: ResMut<GuiState>,
    mut sp: SpawnParams<'_, '_>,
    mut world: ResMut<PhysicsWorld>,
    mut ticker: ResMut<Ticker>,
    mut pending: ResMut<PendingReconciliation>,
    mut net_stats: ResMut<NetworkStats>,
    time: Res<Time>,
    mut biped_q: ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    networked: Query<(Entity, &NetworkID)>,
    possessed_q: Query<(Entity, &NetworkID), With<Possessed>>,
    mut health_q: Query<(&NetworkID, &mut Health)>,
    camera: Query<Entity, With<Camera3d>>,
    // pitch_pivot_q: Query<Entity, With<PitchPivot>>,
    mut next_state: ResMut<NextState<GameState>>,
    projectile_q: Query<(Entity, &ProjectileState)>,
) {
    // tracks entities spawned this on_message call (before commands flush)
    // (entity, server_tick) so Possess can sync the client ticker
    let mut just_spawned: std::collections::HashMap<NetworkID, (Entity, u64)> = Default::default();
    // resolved once at the start; updated inline as Possess / DespawnCommand arrive
    let mut local_net_id: Option<NetworkID> = possessed_q.single().ok().map(|(_, nid)| nid.clone());
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::Connected => {}
            MsgType::SpawnCommand(cmd) => {
                let net_id = cmd.net_id.clone();
                let server_tick = cmd.server_tick;
                let entity = sp.commands.spawn_empty().id();
                sp.commands.queue(SpawnGameObjectCommand { entity, cmd });
                just_spawned.insert(net_id, (entity, server_tick));
            }
            MsgType::Possess(net_id) => {
                // SpawnCommand and Possess may arrive in the same batch before commands flush,
                // so check just_spawned before falling back to the networked query.
                local_net_id = Some(net_id.clone());
                let result = just_spawned.get(&net_id).copied().or_else(|| {
                    networked
                        .iter()
                        .find(|(_, nid)| **nid == net_id)
                        .map(|(e, _)| (e, ticker.tick))
                });
                if let Some((entity, server_tick)) = result {
                    // strip Possessed from the previous pawn so Added<Possessed> fires
                    // cleanly on the new one and interact/camera systems don't double-fire
                    for (old, _) in possessed_q.iter() {
                        if old != entity {
                            sp.commands.entity(old).remove::<Possessed>();
                        }
                    }
                    // sync client tick to server so reconciliation replay covers the right range
                    ticker.tick = server_tick;
                    sp.commands.entity(entity).insert(Possessed::new(128));
                }
            }
            MsgType::DespawnCommand(net_id) => {
                let is_local = local_net_id.as_ref() == Some(&net_id);
                for (entity, nid) in networked.iter() {
                    if *nid == net_id {
                        if is_local {
                            // Detach the camera before despawning so the hierarchy
                            // doesn't take it with it. The server will re-spawn us.
                            if let Ok(cam) = camera.single() {
                                if let Ok(mut entity) = sp.commands.get_entity(cam) {
                                    entity.remove_parent_in_place();
                                }
                            }
                            // Despawn held weapon viewmodels explicitly so the
                            // recursive pawn despawn doesn't hit them a second time.
                            if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
                                for ent in [slots.primary.1.take(), slots.pocket.1.take()] {
                                    if let Some(w) = ent {
                                        if let Ok(mut entity) = sp.commands.get_entity(w) {
                                            entity.despawn();
                                        }
                                    }
                                }
                                slots.primary.0 = None;
                                slots.pocket.0 = None;
                            }
                            local_net_id = None;
                        } else if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
                            // if this was a weapon viewmodel in a slot, clear the slot
                            slots.remove_by_net_id(&net_id);
                        }
                        if let Ok(mut entity_commands) = sp.commands.get_entity(entity) {
                            entity_commands.despawn();
                        }
                        break;
                    }
                }
            }
            MsgType::Disconnected => {
                next_state.set(GameState::MainMenu);
            }
            MsgType::WeaponPickup(weapon_id, carrier_net_id) => {
                let is_local = local_net_id.as_ref() == Some(&carrier_net_id);
                let weapon_entity = networked
                    .iter()
                    .find(|(_, nid)| *nid == &weapon_id)
                    .map(|(e, _)| e);
                let Some(weapon_entity) = weapon_entity else {
                    continue;
                };
                world.set_body_enabled(weapon_entity, false);
                if is_local {
                    let (slot_result, pivot_e) =
                        if let Ok((mut slots, biped)) = biped_q.p0().single_mut() {
                            let result = if slots.primary.0.is_none() {
                                slots.primary = (Some(weapon_id.clone()), Some(weapon_entity));
                                Some((true, None))
                            } else if slots.pocket.0.is_none() {
                                let prev = slots.active().1; // hide whatever is currently shown
                                slots.pocket = (Some(weapon_id.clone()), Some(weapon_entity));
                                slots.active_primary = false;
                                Some((false, prev))
                            } else {
                                None
                            };
                            (result, biped.pitch_pivot)
                        } else {
                            (None, None)
                        };
                    if let Some((is_primary, prev_to_hide)) = slot_result {
                        if let Some(prev) = prev_to_hide {
                            sp.commands.entity(prev).insert(Visibility::Hidden);
                        }
                        if let Some(parent) = camera.single().ok().or(pivot_e) {
                            sp.commands
                                .entity(weapon_entity)
                                .remove::<(RigidBodyHandleComponent, Interactable)>()
                                .set_parent_in_place(parent)
                                .insert(viewmodel_offset(is_primary))
                                .insert(Visibility::Inherited);
                        }
                    }
                } else {
                    // attach to carrier's pitch pivot so the weapon moves with them
                    let carrier = networked
                        .iter()
                        .find(|(_, nid)| *nid == &carrier_net_id)
                        .map(|(e, _)| e);
                    let pivot_e = {
                        let q = biped_q.p1();
                        carrier.and_then(|e| q.get(e).ok().and_then(|b| b.pitch_pivot))
                    };
                    sp.commands.entity(weapon_entity).remove::<Interactable>();
                    if let Some(pivot) = pivot_e {
                        sp.commands
                            .entity(weapon_entity)
                            .set_parent_in_place(pivot)
                            .insert(viewmodel_offset(true));
                    }
                }
            }
            MsgType::WeaponDrop(weapon_id, carrier_id, drop_pos) => {
                let is_carrier = local_net_id.as_ref() == Some(&carrier_id);
                let weapon_entity = networked
                    .iter()
                    .find(|(_, nid)| *nid == &weapon_id)
                    .map(|(e, _)| e);
                let Some(weapon_entity) = weapon_entity else {
                    continue;
                };
                world.teleport_body(weapon_entity, drop_pos);
                world.set_body_enabled(weapon_entity, true);
                if is_carrier {
                    if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
                        slots.remove_by_net_id(&weapon_id);
                    }
                    let handle = world.entity_to_handle.get(&weapon_entity).copied();
                    sp.commands
                        .entity(weapon_entity)
                        .remove_parent_in_place()
                        .insert((Interactable { range: 2.0 }, Visibility::Inherited));
                    if let Some(h) = handle {
                        sp.commands
                            .entity(weapon_entity)
                            .insert(RigidBodyHandleComponent(h));
                    }
                } else {
                    sp.commands
                        .entity(weapon_entity)
                        .insert((Interactable { range: 2.0 }, Visibility::Inherited));
                }
            }
            MsgType::HitResult(_, _, _) => {}
            MsgType::ProjectileConfirm { temp_id, net_id } => {
                // find the locally predicted projectile and attach its server-assigned NetworkID
                for (entity, state) in projectile_q.iter() {
                    if state.temp_id == temp_id {
                        sp.commands.entity(entity).insert(net_id);
                        break;
                    }
                }
            }
            MsgType::HealthUpdate(net_id, current) => {
                for (nid, mut health) in health_q.iter_mut() {
                    if *nid == net_id {
                        health.current = current;
                        break;
                    }
                }
            }
            MsgType::Pong(text) => {
                debug_println!("Client: Got PONG \"{text}\"");
                gui.push_log(format!("pong: {text}"));
            }
            MsgType::ChatMessage(sender, text) => {
                gui.push_log(format!("[{sender}] {text}"));
            }
            MsgType::TimePong(bits) => {
                net_stats.record_pong(bits, time.elapsed_secs_f64());
            }
            // Keep only the newest snapshot; reconciliation happens next FixedPreUpdate.
            MsgType::State(st) => {
                net_stats.record_state_tick(ticker.tick, st.tick, common::config::FIXED_TICK_RATE);
                pending.0 = Some(st);
            }
            MsgType::FileData(name, compressed) => {
                if name == "map.scn.ron" {
                    // store compressed bytes; apply_pending_map_scene will decompress + load
                    sp.commands.insert_resource(PendingMapScene(compressed));
                } else if name == "gametype.lua" {
                    match zstd::stream::decode_all(compressed.as_slice()) {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(src) => {
                                sp.commands.insert_resource(scripting::ScriptConfig {
                                    path: String::new(),
                                    is_server: false,
                                    source: Some(src),
                                });
                            }
                            Err(e) => eprintln!("FileData: gametype.lua not valid utf8: {e}"),
                        },
                        Err(e) => eprintln!("FileData: failed to decompress gametype.lua: {e}"),
                    }
                }
            }
            MsgType::FlashlightState(net_id, on) => {
                // Local player is updated immediately by toggle_flashlight; skip to avoid flicker.
                if local_net_id.as_ref() == Some(&net_id) {
                    continue;
                }
                if let Some((entity, _)) = networked.iter().find(|(_, nid)| *nid == &net_id) {
                    if let Ok(children) = sp.entity_children.get(entity) {
                        for child in children.iter() {
                            if let Ok(mut vis) = sp.lights.get_mut(child) {
                                *vis = if on {
                                    Visibility::Inherited
                                } else {
                                    Visibility::Hidden
                                };
                            }
                        }
                    }
                }
            }
            other => debug_println!("Client: Got unhandled message: {other:?}"),
        }
    }
}

/// Copies the pending server snapshot into LastServerState before reconcile consumes it.
fn snapshot_server_state(pending: Res<PendingReconciliation>, mut last: ResMut<LastServerState>) {
    if let Some(st) = &pending.0 {
        last.0 = Some(st.clone());
    }
}

// /// Draws a point gizmo at each body position from the latest server state.
// fn draw_server_state(last: Res<LastServerState>, mut gizmos: Gizmos) {
//     let Some(state) = &last.0 else { return };
//     for body in state.bodies.values() {
//         let pos: Vec3 = body.position.into();
//         gizmos.sphere(pos, 0.15, Color::srgb(1.0, 0.2, 0.2));
//     }
// }

fn debug_render_on(s: Res<Settings>) -> bool {
    s.debug_render
}

//
// /// Scroll wheel switches the active weapon slot and toggles viewmodel visibility.
// fn switch_weapon_slot(
//     scroll: Res<AccumulatedMouseScroll>,
//     mut pawn: Query<&mut WeaponSlots, With<Possessed>>,
//     mut visibility: Query<&mut Visibility>,
// ) {
//     let delta: f32 = scroll.delta.y;
//     if delta == 0.0 { return; }
//     let Ok(mut slots) = pawn.single_mut() else { return };
//     let prev = slots.active;
//     slots.active = if delta > 0.0 {
//         (slots.active + 1) % 2
//     } else {
//         slots.active.checked_sub(1).unwrap_or(1)
//     };
//     if slots.active == prev { return; }
//     if let Some(e) = slots.slots[prev].1 {
//         if let Ok(mut vis) = visibility.get_mut(e) { *vis = Visibility::Hidden; }
//     }
//     if let Some(e) = slots.slots[slots.active].1 {
//         if let Ok(mut vis) = visibility.get_mut(e) { *vis = Visibility::Inherited; }
//     }
// }
use bevy::core_pipeline::Skybox;
