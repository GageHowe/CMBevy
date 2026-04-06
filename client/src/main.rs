// client executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;
use camera::spawn_camera;
use common::debug_println;
pub use common::game_state::GameState;
use game_objects::GameObjectsPlugin;
use game_objects::pawn::vehicle::draw_driver_seat_debug;
use game_objects::pawn::{self, *};
use game_objects::projectile::hail_mary::HailMaryProjectile;
use game_objects::projectile::rifle::*;
use game_objects::projectile::rpg::RpgProjectile;
use game_objects::projectile::*;
use game_objects::weapon::WeaponPlugin;
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
use session::{ClientSessionPlugin, HostedServer, PendingExit, ServerAddr, SinglePlayerConfig};

use game_objects::components::planet::draw_planet_radii;
use game_objects::level::{
    LevelPlugin, MapMeta, apply_pending_map_scene, cleanup_level, load_level_scene,
};
use master_plugin::MasterPlugin;
use physics::physics_world::{step_physics, sync_physics_visual};
use settings::{Settings, SettingsPlugin};
use steam::SteamworksPlugin;
// use game_objects::pawn::biped::draw_biped_debug; // don't do debug for bipeds for now
mod settings;
mod sound;
mod steam;
use sound::SoundPlugin;

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
    app.add_systems(FixedPostUpdate, session::on_message);

    app.add_systems(
        FixedLast,
        session::snapshot_server_state.run_if(
            in_state(GameState::Multiplayer).and(resource_changed::<PendingReconciliation>),
        ),
    );

    // (tick increment is FixedLast)

    app.add_systems(Update, load_level_scene.run_if(resource_added::<MapMeta>));
    app.add_systems(Update, apply_pending_map_scene);
    app.add_systems(Update, draw_planet_radii.run_if(gameplay_overlay_on));
    app.add_systems(Update, draw_driver_seat_debug.run_if(debug_render_on));
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

    debug_println!("starting client...\n");
    app.run();
}

/// handles messages coming in from the server
/// called by quic on FixedPostUpdate
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

fn gameplay_overlay_on(s: Res<Settings>) -> bool {
    !s.cinematic_mode
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
