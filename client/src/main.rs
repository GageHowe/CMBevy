// client executable
#![allow(linker_messages)]
// WARNING: don't put common dependencies here, put them in MasterPlugin

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use audio::SoundPlugin;
use auto_exposure_debug::AutoExposureDebugPlugin;
use bevy::{
    asset::AssetMetaCheck,
    log::{Level, LogPlugin},
    pbr::DefaultOpaqueRendererMethod,
    post_process::auto_exposure::AutoExposurePlugin,
    prelude::*,
    window::PresentMode,
};
use camera::spawn_camera;
pub use common::game_state::GameState;
use gameplay::{
    components::{gravity::draw_gravity_radii, snap::draw_snap_radii},
    level::{cleanup_level, draw_script_zone_debug},
    pawn::{self, biped::draw_melee_debug, mount::draw_mount_debug, *},
    projectile::*,
};
use hosting::cleanup_before_app_exit;
use particles_plugin::prelude::GPUParticlesPlugin;
use reconciliation::*;
use tick_sync::TickSyncPlugin;
use ui::{UIPlugin, window::WindowSettingsPlugin};

mod auto_exposure_debug;
mod camera;
mod hosting;
mod menu;
mod outline;
mod reconciliation;
mod sound;
mod tick_sync;
mod ui;
use gameplay::session::*;
use master_plugin::{MasterPlugin, register_asset_pak};
use menu::MenuPlugin;
use outline::OutlinePlugin;
use physics::physics_world::{step_physics, sync_physics_visual};
use settings::{Settings, SettingsPlugin};
use steam::SteamworksPlugin;
// use gameplay::pawn::biped::draw_biped_debug; // don't do debug for bipeds for now
mod settings;
mod steam;

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
    app.insert_resource(DefaultOpaqueRendererMethod::deferred());
    register_asset_pak(&mut app);

    app.add_plugins(
        DefaultPlugins
            .build()
            .set(AssetPlugin {
                file_path: common::config::asset_dir().to_string_lossy().into_owned(),
                meta_check: AssetMetaCheck::Never,
                watch_for_changes_override: Some(cfg!(debug_assertions)),
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

    app.add_plugins((AutoExposurePlugin, AutoExposureDebugPlugin, OutlinePlugin))
        .init_state::<GameState>()
        .init_state::<UiState>()
        .add_plugins(MasterPlugin)
        .add_plugins(SteamworksPlugin) // prints steam info on Startup
        .add_plugins(SettingsPlugin)
        .add_plugins(WindowSettingsPlugin)
        .add_plugins(UIPlugin)
        .add_plugins(MenuPlugin)
        .add_plugins(GPUParticlesPlugin)
        .add_plugins(SoundPlugin)
        .add_plugins(sound::ClientSoundPlugin)
        .add_plugins(ClientSessionPlugin {
            main_menu: GameState::NotPlaying,
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
        .init_resource::<SinglePlayerConfig>()
        .add_systems(
            FixedUpdate,
            step_physics.run_if(
                in_state(GameState::SinglePlayer).or_else(in_state(GameState::Multiplayer)),
            ),
        )
        .add_systems(
            Update,
            sync_physics_visual.run_if(
                in_state(GameState::SinglePlayer).or_else(in_state(GameState::Multiplayer)),
            ),
        )
        .add_systems(Startup, spawn_camera)
        .add_systems(Last, cleanup_before_app_exit);
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
        FixedUpdate,
        (draw_projectile_debug, draw_projectile_raycast_debug)
            .after(step_physics)
            .run_if(debug_render_on)
            .run_if(in_state(GameState::SinglePlayer).or_else(in_state(GameState::Multiplayer))),
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
