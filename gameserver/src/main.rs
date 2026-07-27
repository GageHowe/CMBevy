// server executable
#![allow(linker_messages)]

use std::{io, net::SocketAddr, process::exit};

use bevy::{
    asset::AssetMetaCheck,
    log::{Level, LogPlugin},
    prelude::*,
};
use common::{game_state::SimState, slow_update::SlowUpdate};
use gameplay::session::{ActiveConnections, ServerSessionPlugin};
use http_common::RegisterRequest;
use master_plugin::{MasterPlugin, register_asset_pak};
use physics::physics_world::*;

fn parse_args() -> io::Result<(SocketAddr, String, String, Option<RegisterRequest>)> {
    let mut addr = common::config::SERVER_BIND_ADDRESS.to_string();
    let mut map = format!(
        "maps/{}",
        first_asset_name("maps", "ron").ok_or_else(|| io::Error::new(
            io::ErrorKind::NotFound,
            "no maps found in assets/maps"
        ))?
    );
    let mut gametype = gameplay::level::default_asset_dir()
        .join("gametypes")
        .join(first_asset_name("gametypes", "lua").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no gametypes found in assets/gametypes",
            )
        })?)
        .to_string_lossy()
        .into_owned();
    let mut advertise_name = None;
    let mut advertise_max_players = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                if let Some(p) = args.next().and_then(|p| p.parse::<u16>().ok()) {
                    addr = format!("0.0.0.0:{p}");
                }
            }
            "--map" => {
                if let Some(v) = args.next() {
                    map = v;
                }
            }
            "--gametype" => {
                if let Some(v) = args.next() {
                    gametype = v;
                }
            }
            "--advertise-name" => advertise_name = args.next(),
            "--advertise-max-players" => {
                advertise_max_players = args.next().and_then(|v| v.parse::<u8>().ok())
            }
            _ => {}
        }
    }
    let bind_addr: SocketAddr = addr.parse().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid bind address '{addr}': {err}"),
        )
    })?;
    let advertise = advertise_name.map(|name| RegisterRequest {
        quic_port: bind_addr.port(),
        name,
        max_players: advertise_max_players.unwrap_or(8),
    });
    Ok((bind_addr, map, gametype, advertise))
}

/// too complicated, TODO remove
fn first_asset_name(dir: &str, ext: &str) -> Option<String> {
    let asset_dir = gameplay::level::default_asset_dir();
    let mut names: Vec<String> = std::fs::read_dir(asset_dir.join(dir))
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|x| x == ext))
        .filter_map(|entry| {
            entry
                .path()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names.into_iter().next()
}

/// launch a software thread which responds to UDP "discover" probes so LAN clients can find this server.
fn start_lan_discovery(quic_port: u16) {
    let port_str = quic_port.to_string();
    std::thread::spawn(move || {
        let Ok(sock) =
            std::net::UdpSocket::bind(format!("0.0.0.0:{}", common::config::LAN_DISCOVERY_PORT))
        else {
            exit(1);
        };
        let mut buf = [0u8; 16];
        loop {
            let Ok((n, from)) = sock.recv_from(&mut buf) else {
                continue;
            };
            if &buf[..n] == b"discover" {
                let _ = sock.send_to(port_str.as_bytes(), from);
            }
        }
    });
}

/*

Args

--map <path> open the server to the map at this path
TODO


 */
fn main() {
    let (bind_addr, map_path, gametype_path, advertise) = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    println!("binding to {bind_addr}\nmap={map_path}\ngametype={gametype_path}");
    start_lan_discovery(bind_addr.port());

    let mut app = App::new();
    register_asset_pak(&mut app);
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::state::app::StatesPlugin)
        .add_plugins(bevy::asset::AssetPlugin {
            file_path: gameplay::level::default_asset_dir()
                .to_string_lossy()
                .into_owned(),
            meta_check: AssetMetaCheck::Never,
            ..default()
        })
        .add_plugins(bevy::world_serialization::WorldSerializationPlugin) // needed to register DynamicWorld asset + RON loader
        .add_plugins(LogPlugin {
            level: Level::ERROR,
            ..default()
        });

    app.add_plugins(MasterPlugin);
    app.add_systems(FixedPreUpdate, gameplay::session::on_message);
    app.add_systems(SlowUpdate, pause_empty_server);
    app.add_systems(
        FixedUpdate,
        (step_physics, sync_physics_to_transforms)
            .chain()
            .in_set(common::game_state::SimulationSystems),
    );
    app.add_plugins(ServerSessionPlugin {
        bind_addr,
        map_path,
        gametype_path,
        advertise,
    });
    println!("starting server...\n");
    app.run();
}

fn pause_empty_server(
    connections: Res<ActiveConnections>,
    state: Res<bevy::prelude::State<SimState>>,
    mut next_state: ResMut<NextState<SimState>>,
) {
    let next = if connections.0.is_empty() {
        SimState::Paused
    } else {
        SimState::Playing
    };
    if *state.get() != next {
        next_state.set(next);
    }
}

#[cfg(test)]
mod tests;
