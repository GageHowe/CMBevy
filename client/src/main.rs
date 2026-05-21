// client executable
// WARNING: don't put common dependencies here, put them in MasterPlugin

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use auto_exposure_debug::AutoExposureDebugPlugin;
use bevy::{
    log::{Level, LogPlugin},
    post_process::auto_exposure::AutoExposurePlugin,
    prelude::*,
    window::PresentMode,
};
use bevy_hanabi_plugin::prelude::HanabiEffectsPlugin;
use camera::spawn_camera;
use color_compression::ColorCompressionPlugin;
pub use common::game_state::GameState;
use game_objects::{
    pawn::{self, biped::draw_melee_debug, mount::draw_mount_debug, *},
    projectile::{
        coil_launcher::CoilLauncherProjectile, hail_mary::HailMaryProjectile,
        lobber::LobberProjectile, rifle::*, *,
    },
};
use reconciliation::*;
use tick_sync::TickSyncPlugin;
use ui::{UIPlugin, window::WindowSettingsPlugin};

mod auto_exposure_debug;
mod camera;
mod color_compression;
mod fullscreen_post_process;
mod menu;
mod outline;
mod raytrace;
mod reconciliation;
mod tick_sync;
mod ui;
use game_objects::{
    components::{
        gravity::draw_gravity_radii, snap::draw_snap_radii,
    },
    level::{cleanup_level, draw_script_zone_debug},
};
use master_plugin::MasterPlugin;
use menu::MenuPlugin;
use outline::OutlinePlugin;
use physics::physics_world::{step_physics, sync_physics_visual};
use raytrace::RaytraceTogglePlugin;
use session::{
    ClientSessionPlugin, HostedServer, PendingExit, ServerAddr, SinglePlayerConfig,
    draw_server_state,
};
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
    match common::config::SERVER_BIND_ADDRESS.parse() {
        Ok(addr) => addr,
        Err(err) => {
            error!(
                "invalid SERVER_BIND_ADDRESS '{}': {err}",
                common::config::SERVER_BIND_ADDRESS
            );
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 42070)
        }
    }
}

fn main() {
    let server_addr = parse_server_addr();
    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::asset::io::web::WebAssetPlugin>()
            .set(AssetPlugin {
                file_path: common::config::asset_dir().to_string_lossy().into_owned(),
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

    app.add_plugins((
        AutoExposurePlugin,
        AutoExposureDebugPlugin,
        OutlinePlugin,
        ColorCompressionPlugin,
        RaytraceTogglePlugin,
    ))
    .init_state::<GameState>()
    .init_state::<UiState>()
    .add_plugins(MasterPlugin)
    .add_plugins(SteamworksPlugin) // prints steam info on Startup
    .add_plugins(SettingsPlugin)
    .add_plugins(WindowSettingsPlugin)
    .add_plugins(UIPlugin)
    .add_plugins(MenuPlugin)
    .add_plugins(HanabiEffectsPlugin)
    .add_plugins(SoundPlugin)
    .add_plugins(ClientSessionPlugin {
        main_menu: GameState::MainMenu,
        single_player: GameState::SinglePlayer,
        multiplayer: GameState::Multiplayer,
    })
    .add_plugins(ReconciliationPlugin::<GameState>::new(
        GameState::Multiplayer,
    ))
    .add_plugins(TickSyncPlugin(GameState::Multiplayer))
    .insert_resource(ServerAddr {
        addr: server_addr,
        lobby_id: None,
    })
    .init_resource::<PendingExit>()
    .init_resource::<SinglePlayerConfig>()
    .init_resource::<HostedServer>()
    // PendingHullColliders now managed by LevelPlugin
    .add_systems(
        FixedUpdate,
        step_physics.run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))),
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

    app.add_systems(Update, draw_gravity_radii.run_if(gameplay_overlay_on));
    app.add_systems(Update, draw_snap_radii.run_if(gameplay_overlay_on));
    app.add_systems(Update, draw_script_zone_debug.run_if(debug_render_on));
    app.add_systems(Update, draw_mount_debug.run_if(debug_render_on));
    app.add_systems(Update, draw_melee_debug.run_if(debug_render_on));
    app.add_systems(
        Update,
        draw_server_state
            .run_if(debug_render_on)
            .run_if(in_state(GameState::Multiplayer)),
    );
    app.add_systems(
        FixedUpdate,
        (
            draw_projectile_debug::<HailMaryProjectile>(Color::srgba(1.0, 0.3, 0.1, 0.9)),
            draw_projectile_debug::<LobberProjectile>(Color::srgba(1.0, 0.5, 0.2, 0.9)),
            draw_projectile_debug::<CoilLauncherProjectile>(Color::srgba(1.0, 0.9, 0.2, 0.9)),
            draw_projectile_debug::<RifleProjectile>(Color::srgba(1.0, 0.9, 0.2, 0.9)),
            draw_projectile_raycast_debug,
        )
            .after(step_physics)
            .run_if(debug_render_on)
            .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))),
    );

    info!("starting client...");
    app.run();
}

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
