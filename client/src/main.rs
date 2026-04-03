// client executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;
use camera::spawn_camera;
pub use common::game_state::GameState;
use common::interaction::Interactable;
use common::tick::{NetworkStats, Ticker};
use game_objects::pawn::biped::*;
use game_objects::pawn::vehicle::draw_cockpit_debug;
use game_objects::pawn::{self, *};
use game_objects::projectile::PredictedProjectileMap;
use game_objects::projectile::hail_mary::HailMaryProjectile;
use game_objects::projectile::rifle::*;
use game_objects::projectile::rpg::RpgProjectile;
use game_objects::projectile::*;
use game_objects::weapon::{WeaponPlugin, helpers as weapon_helpers};
use game_objects::{GameObjectsPlugin, NetworkEntityMap, SpawnGameObjectCommand};
use net::{message::*, quic::*};
use physics::physics_world::*;
use reconciliation::*;
use std::net::SocketAddr;
use tick_sync::TickSyncPlugin;
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
use session::{ClientSessionPlugin, LastAckedInputSeq, shutdown_session};

#[derive(Resource)]
pub(crate) struct ServerAddr(pub SocketAddr);

#[derive(Resource, Default)]
pub(crate) struct PendingExit(pub bool);

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

#[derive(bevy::ecs::system::SystemParam)]
struct ClientMessageParams<'w, 's> {
    spawn: SpawnParams<'w, 's>,
    world: ResMut<'w, PhysicsWorld>,
    biped_q: ParamSet<
        'w,
        's,
        (
            Query<'w, 's, (&'static mut WeaponSlots, &'static BipedPawnComponent), With<Possessed>>,
            Query<'w, 's, &'static BipedPawnComponent>,
        ),
    >,
    networked: Res<'w, NetworkEntityMap>,
    health_q: Query<'w, 's, &'static mut Health>,
    camera: Query<'w, 's, Entity, With<Camera3d>>,
    projectile_q: Query<'w, 's, (Entity, &'static ProjectileState)>,
    predicted_projectiles: ResMut<'w, PredictedProjectileMap>,
}
use common::debug_println;
use game_objects::health::Health;
use game_objects::level::{
    LevelPlugin, MapMeta, PendingMapScene, apply_pending_map_scene, cleanup_level, load_level_scene,
};
use game_objects::components::planet::draw_planet_radii;
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

    match bevy_steamworks::SteamworksPlugin::init_app(3526510u32) {
        Ok(steam) => {
            app.add_plugins(steam);
        }
        Err(err) => {
            warn!("Steam init failed: {err}");
        }
    }

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
        .add_plugins(SteamworksPlugin) // prints steam info on Startup
        .add_plugins(SettingsPlugin)
        .init_state::<GameState>()
        .init_state::<UiState>()
        .add_plugins(WindowSettingsPlugin)
        .add_plugins(UIPlugin)
        .add_plugins(MenuPlugin)
        .add_plugins(GameObjectsPlugin)
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
        .init_resource::<PendingExit>()
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
    //   on_message/send_chat
    app.add_systems(FixedPostUpdate, on_message);

    app.add_systems(
        FixedLast,
        session::snapshot_server_state.run_if(
            in_state(GameState::Multiplayer).and(resource_changed::<PendingReconciliation>),
        ),
    );

    // (tick increment is FixedLast)

    app.add_systems(Update, load_level_scene.run_if(resource_added::<MapMeta>));
    app.add_systems(Update, apply_pending_map_scene);
    app.add_systems(Update, draw_planet_radii.run_if(debug_render_on));
    app.add_systems(Update, draw_cockpit_debug.run_if(debug_render_on));
    app.add_systems(
        FixedUpdate,
        (
            draw_projectile_debug::<HailMaryProjectile>(Color::srgba(1.0, 0.3, 0.1, 0.9)),
            draw_projectile_debug::<RpgProjectile>(Color::srgba(1.0, 0.5, 0.2, 0.9)),
            draw_projectile_debug::<RifleProjectile>(Color::srgba(1.0, 0.9, 0.2, 0.9)),
        )
            .after(step_physics)
            .run_if(debug_render_on)
            .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))),
    );

    app.add_systems(Last, cleanup_before_app_exit);
    app.add_systems(
        Update,
        exit_after_returning_to_menu.run_if(in_state(GameState::MainMenu)),
    );

    debug_println!("starting client...\n");
    app.run();
}

fn cleanup_before_app_exit(
    mut exits: MessageReader<AppExit>,
    mut quic: Option<ResMut<QuicManager>>,
    mut pending: Option<ResMut<PendingReconciliation>>,
    mut hosted: ResMut<HostedServer>,
) {
    if exits.read().next().is_none() {
        return;
    }
    shutdown_session(quic.as_deref_mut(), pending.as_deref_mut(), &mut hosted);
}

fn exit_after_returning_to_menu(pending_exit: Res<PendingExit>, mut exit: MessageWriter<AppExit>) {
    if pending_exit.0 {
        exit.write(AppExit::Success);
    }
}

/// handles messages coming in from the server
/// called by quic on FixedPostUpdate
fn on_message(
    quic: Option<ResMut<QuicManager>>,
    mut gui: ResMut<GuiState>,
    mut mp: ClientMessageParams<'_, '_>,
    mut ticker: ResMut<Ticker>,
    mut pending: ResMut<PendingReconciliation>,
    mut net_stats: ResMut<NetworkStats>,
    mut last_acked_input_seq: ResMut<LastAckedInputSeq>,
    time: Res<Time>,
    possessed_q: Query<(Entity, &NetworkID), With<Possessed>>,
    // pitch_pivot_q: Query<Entity, With<PitchPivot>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Some(mut quic) = quic else {
        return;
    };
    // tracks entities spawned this on_message call (before commands flush)
    // (entity, server_tick) so Possess can sync the client ticker
    let mut just_spawned: std::collections::HashMap<NetworkID, (Entity, u64)> = Default::default();
    // resolved once at the start; updated inline as Possess / DespawnCommand arrive
    let mut local_net_id: Option<NetworkID> = possessed_q.single().ok().map(|(_, nid)| nid.clone());
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::Connected => {}
            MsgType::SpawnCommand(cmd) => {
                handle_spawn_command(&mut mp.spawn.commands, &mut just_spawned, cmd);
            }
            MsgType::Possess(net_id) => {
                handle_possess(
                    net_id,
                    &mut local_net_id,
                    &just_spawned,
                    &mp.networked,
                    &possessed_q,
                    &mut mp.spawn.commands,
                    &mut ticker,
                );
            }
            MsgType::DespawnCommand(net_id) => {
                handle_despawn(
                    &net_id,
                    &mut local_net_id,
                    &mp.networked,
                    &mp.camera,
                    &mut mp.biped_q,
                    &mut mp.spawn.commands,
                );
            }
            MsgType::Disconnected => {
                next_state.set(GameState::MainMenu);
            }
            MsgType::WeaponPickup(weapon_id, carrier_net_id) => {
                handle_weapon_pickup(
                    &weapon_id,
                    &carrier_net_id,
                    local_net_id.as_ref(),
                    &mp.networked,
                    &mp.camera,
                    &mut mp.biped_q,
                    &mut mp.spawn.commands,
                    &mut mp.world,
                );
            }
            MsgType::WeaponDrop(weapon_id, carrier_id, drop_pos) => {
                handle_weapon_drop(
                    &weapon_id,
                    &carrier_id,
                    drop_pos,
                    local_net_id.as_ref(),
                    &mp.networked,
                    &mut mp.biped_q,
                    &mut mp.spawn.commands,
                    &mut mp.world,
                );
            }
            MsgType::HitResult(_, _, _) => {}
            MsgType::ProjectileConfirm { temp_id, net_id } => {
                handle_projectile_confirm(
                    temp_id,
                    net_id,
                    &mut mp.predicted_projectiles,
                    &mp.projectile_q,
                    &mut mp.spawn.commands,
                );
            }
            MsgType::HealthUpdate(net_id, current) => {
                handle_health_update(&net_id, current, &mp.networked, &mut mp.health_q);
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
                if st.last_input_seq >= last_acked_input_seq.0 {
                    last_acked_input_seq.0 = st.last_input_seq;
                    pending.0 = Some(st);
                }
            }
            MsgType::FileData(name, compressed) => {
                handle_file_data(name, compressed, &mut mp.spawn.commands);
            }
            MsgType::FlashlightState(net_id, on) => {
                handle_flashlight_state(
                    &net_id,
                    on,
                    local_net_id.as_ref(),
                    &mp.networked,
                    &mp.spawn.entity_children,
                    &mut mp.spawn.lights,
                );
            }
            other => debug_println!("Client: Got unhandled message: {other:?}"),
        }
    }
}

fn find_networked_entity(networked: &NetworkEntityMap, net_id: &NetworkID) -> Option<Entity> {
    networked.get(net_id)
}

fn handle_spawn_command(
    commands: &mut Commands,
    just_spawned: &mut std::collections::HashMap<NetworkID, (Entity, u64)>,
    cmd: SpawnCommand,
) {
    let net_id = cmd.net_id.clone();
    let server_tick = cmd.server_tick;
    let entity = commands.spawn_empty().id();
    commands.queue(SpawnGameObjectCommand { entity, cmd });
    just_spawned.insert(net_id, (entity, server_tick));
}

fn handle_possess(
    net_id: NetworkID,
    local_net_id: &mut Option<NetworkID>,
    just_spawned: &std::collections::HashMap<NetworkID, (Entity, u64)>,
    networked: &NetworkEntityMap,
    possessed_q: &Query<(Entity, &NetworkID), With<Possessed>>,
    commands: &mut Commands,
    ticker: &mut Ticker,
) {
    *local_net_id = Some(net_id.clone());
    let result = just_spawned
        .get(&net_id)
        .copied()
        .or_else(|| find_networked_entity(networked, &net_id).map(|entity| (entity, ticker.tick)));
    let Some((entity, server_tick)) = result else {
        return;
    };
    for (old, _) in possessed_q.iter() {
        if old != entity {
            commands.entity(old).remove::<Possessed>();
        }
    }
    ticker.tick = server_tick;
    commands.entity(entity).insert(Possessed::new(128));
}

fn handle_despawn(
    net_id: &NetworkID,
    local_net_id: &mut Option<NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    commands: &mut Commands,
) {
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let is_local = local_net_id.as_ref() == Some(net_id);
    if is_local {
        if let Ok(cam) = camera.single() {
            if let Ok(mut entity) = commands.get_entity(cam) {
                entity.remove_parent_in_place();
            }
        }
        if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
            for ent in [slots.primary.1.take(), slots.pocket.1.take()] {
                if let Some(w) = ent {
                    if let Ok(mut entity) = commands.get_entity(w) {
                        entity.despawn();
                    }
                }
            }
            slots.primary.0 = None;
            slots.pocket.0 = None;
        }
        *local_net_id = None;
    } else if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
        slots.remove_by_net_id(net_id);
    }
    if let Ok(mut entity_commands) = commands.get_entity(entity) {
        entity_commands.despawn();
    }
}

fn handle_weapon_pickup(
    weapon_id: &NetworkID,
    carrier_net_id: &NetworkID,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Some(weapon_entity) = find_networked_entity(networked, weapon_id) else {
        return;
    };
    world.set_body_enabled(weapon_entity, false);
    if local_net_id == Some(carrier_net_id) {
        let (slot_result, pivot_e) = if let Ok((mut slots, biped)) = biped_q.p0().single_mut() {
            (
                weapon_helpers::assign_local_pickup_slot(
                    &mut slots,
                    weapon_id.clone(),
                    weapon_entity,
                ),
                biped.pitch_pivot,
            )
        } else {
            (None, None)
        };
        if let Some((is_primary, prev_to_hide)) = slot_result {
            if let Some(prev) = prev_to_hide {
                commands.entity(prev).insert(Visibility::Hidden);
            }
            if let Some(parent) = camera.single().ok().or(pivot_e) {
                weapon_helpers::attach_local_viewmodel(commands, weapon_entity, parent, is_primary);
            }
        }
        return;
    }
    let carrier = find_networked_entity(networked, carrier_net_id);
    let pivot_e = {
        let q = biped_q.p1();
        carrier.and_then(|entity| q.get(entity).ok().and_then(|b| b.pitch_pivot))
    };
    if let Some(pivot) = pivot_e {
        weapon_helpers::attach_remote_viewmodel(commands, weapon_entity, pivot);
    }
}

fn handle_weapon_drop(
    weapon_id: &NetworkID,
    carrier_id: &NetworkID,
    drop_pos: Vec3,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Some(weapon_entity) = find_networked_entity(networked, weapon_id) else {
        return;
    };
    world.teleport_body(weapon_entity, drop_pos);
    world.set_body_enabled(weapon_entity, true);
    if local_net_id == Some(carrier_id) {
        if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
            slots.remove_by_net_id(weapon_id);
        }
        weapon_helpers::detach_viewmodel(commands, world, weapon_entity);
    } else {
        commands
            .entity(weapon_entity)
            .insert((Interactable { range: 2.0 }, Visibility::Inherited));
    }
}

fn handle_projectile_confirm(
    temp_id: u32,
    net_id: NetworkID,
    predicted_projectiles: &mut PredictedProjectileMap,
    projectile_q: &Query<(Entity, &ProjectileState)>,
    commands: &mut Commands,
) {
    if let Some(entity) = predicted_projectiles.get(temp_id) {
        predicted_projectiles.remove_temp_id(temp_id);
        commands.entity(entity).insert(net_id);
        return;
    }
    for (entity, state) in projectile_q.iter() {
        if state.temp_id == temp_id {
            predicted_projectiles.remove_temp_id(temp_id);
            commands.entity(entity).insert(net_id);
            break;
        }
    }
}

fn handle_health_update(
    net_id: &NetworkID,
    current: f32,
    networked: &NetworkEntityMap,
    health_q: &mut Query<&mut Health>,
) {
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let Ok(mut health) = health_q.get_mut(entity) else {
        return;
    };
    health.current = current;
}

fn handle_file_data(name: String, compressed: Vec<u8>, commands: &mut Commands) {
    if name == "map.scn.ron" {
        commands.insert_resource(PendingMapScene(compressed));
        return;
    }
    if name != "gametype.lua" {
        return;
    }
    match zstd::stream::decode_all(compressed.as_slice()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(src) => {
                commands.insert_resource(scripting::ScriptConfig {
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

fn handle_flashlight_state(
    net_id: &NetworkID,
    on: bool,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    entity_children: &Query<&Children>,
    lights: &mut Query<&mut Visibility, With<SpotLight>>,
) {
    if local_net_id == Some(net_id) {
        return;
    }
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let Ok(children) = entity_children.get(entity) else {
        return;
    };
    for child in children.iter() {
        if let Ok(mut vis) = lights.get_mut(child) {
            *vis = if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
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
