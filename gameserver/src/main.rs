// server executable

use std::net::SocketAddr;

use bevy::{
    log::{Level, LogPlugin},
    prelude::*,
};
use game_objects::{level::apply_pending_map_scene, pawn::HeldWeaponMap};
use master_plugin::MasterPlugin;
use physics::physics_world::*;
use session::ServerSessionPlugin;

fn parse_args() -> (SocketAddr, String, String) {
    let mut addr = common::config::SERVER_BIND_ADDRESS.to_string();
    let mut map =
        format!("maps/{}", first_asset_name("maps", "ron").expect("no maps found in assets/maps"));
    let mut gametype = game_objects::level::default_asset_dir()
        .join("gametypes")
        .join(first_asset_name("gametypes", "lua").expect("no gametypes found in assets/gametypes"))
        .to_string_lossy()
        .into_owned();
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
            _ => {}
        }
    }
    (addr.parse().unwrap(), map, gametype)
}

fn first_asset_name(dir: &str, ext: &str) -> Option<String> {
    let asset_dir = game_objects::level::default_asset_dir();
    let mut names: Vec<String> = std::fs::read_dir(asset_dir.join(dir))
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|x| x == ext))
        .filter_map(|entry| {
            entry.path().file_name().map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names.into_iter().next()
}

/// Responds to UDP "discover" probes so LAN clients can find this server.
fn start_lan_discovery(quic_port: u16) {
    let port_str = quic_port.to_string();
    std::thread::spawn(move || {
        let Ok(sock) =
            std::net::UdpSocket::bind(format!("0.0.0.0:{}", common::config::LAN_DISCOVERY_PORT))
        else {
            return;
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

fn main() {
    let (bind_addr, map_path, gametype_path) = parse_args();
    println!("binding to {bind_addr}\nmap={map_path}\ngametype={gametype_path}");
    start_lan_discovery(bind_addr.port());

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin {
            file_path: game_objects::level::default_asset_dir().to_string_lossy().into_owned(),
            ..default()
        })
        .add_plugins(bevy::scene::ScenePlugin) // needed to register DynamicScene asset + RON loader
        .add_plugins(LogPlugin { level: Level::ERROR, ..default() });

    app.add_plugins(MasterPlugin);
    app.init_resource::<HeldWeaponMap>();
    app.add_systems(Update, apply_pending_map_scene);
    app.add_systems(FixedPreUpdate, session::on_message);
    app.add_systems(FixedUpdate, (step_physics, sync_physics_to_transforms).chain());
    app.add_plugins(ServerSessionPlugin { bind_addr, map_path, gametype_path });
    println!("starting server...\n");
    app.run();
}
