use std::net::Ipv4Addr;

use bevy::{app::AppExit, prelude::*};
use net::quic::QuicManager;
use session::PendingReconciliation;

pub fn available_maps() -> Vec<String> {
    scan_dir(common::config::asset_dir().join("maps"), "ron")
}

/// return a list of Strings denoting available gametype files
pub fn available_gametypes() -> Vec<String> {
    scan_dir(common::config::asset_dir().join("gametypes"), "lua")
}

pub fn gametype_path(name: &str) -> String {
    common::config::asset_dir()
        .join("gametypes")
        .join(format!("{name}.lua"))
        .to_string_lossy()
        .into_owned()
}

pub fn fetch_lan_lobbies() -> Result<Vec<http_common::LobbyInfo>, String> {
    use std::{net::UdpSocket, time::Duration};
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.set_broadcast(true).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(1500)))
        .map_err(|e| e.to_string())?;
    let _ = sock.send_to(
        b"discover",
        format!("255.255.255.255:{}", common::config::LAN_DISCOVERY_PORT),
    );
    let _ = sock.send_to(
        b"discover",
        format!("127.0.0.1:{}", common::config::LAN_DISCOVERY_PORT),
    );
    let mut lobbies = Vec::new();
    let mut buf = [0u8; 16];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Ok(port) = std::str::from_utf8(&buf[..n]).unwrap_or("").parse::<u16>() {
                    lobbies.push(http_common::LobbyInfo {
                        id: String::new(),
                        name: format!("LAN @ {}", from.ip()),
                        host: format!("{}:{}", from.ip(), port),
                        player_count: 0,
                        max_players: 0,
                    });
                }
            }
            Err(_) => break,
        }
    }
    Ok(lobbies)
}

pub fn fetch_remote_lobbies() -> Result<Vec<http_common::LobbyInfo>, String> {
    ureq::get(&format!("{}/lobbies", common::config::BEACON_URL))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())
}

pub fn start_hosted_server(
    port: u16,
    map: &str,
    gametype: &str,
    advertise: Option<http_common::RegisterRequest>,
) -> std::io::Result<()> {
    let preflight = match std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)) {
        Ok(sock) => sock,
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
            kill_local_port_owners(port);
            std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?
        }
        Err(err) => return Err(err),
    };
    drop(preflight);
    spawn_gameserver_terminal(port, map, gametype, advertise)
}

pub fn cleanup_before_app_exit(
    mut exits: MessageReader<AppExit>,
    mut quic: Option<ResMut<QuicManager>>,
    mut pending: Option<ResMut<PendingReconciliation>>,
) {
    if exits.read().next().is_none() {
        return;
    }
    if let Some(quic) = quic.as_deref_mut() {
        quic.disconnect();
        quic.inbound.clear();
    }
    if let Some(pending) = pending.as_deref_mut() {
        pending.0 = None;
    }
}

fn scan_dir(dir: impl AsRef<std::path::Path>, ext: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == ext))
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names
}

fn gameserver_exe() -> std::path::PathBuf {
    let bin = if cfg!(windows) {
        "gameserver.exe"
    } else {
        "gameserver"
    };
    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join(bin));
        if dir.file_name().is_some_and(|name| name == "deps")
            && let Some(parent) = dir.parent()
        {
            candidates.push(parent.join(bin));
        }
    }

    if let Some(workspace_root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        let profile = option_env!("PROFILE").unwrap_or(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        });
        candidates.push(workspace_root.join("target").join(profile).join(bin));
        candidates.push(workspace_root.join("target").join("debug").join(bin));
        candidates.push(workspace_root.join("target").join("release").join(bin));
        candidates.push(workspace_root.join("target").join("profiling").join(bin));
    }

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(if cfg!(windows) {
                "gameserver.exe"
            } else {
                "gameserver"
            })
        })
}

fn spawn_gameserver_terminal(
    port: u16,
    map: &str,
    gametype: &str,
    advertise: Option<http_common::RegisterRequest>,
) -> std::io::Result<()> {
    let exe = gameserver_exe();
    let mut args = vec![
        "--port".to_string(),
        port.to_string(),
        "--map".to_string(),
        map.to_string(),
        "--gametype".to_string(),
        gametype.to_string(),
    ];
    if let Some(req) = advertise {
        args.push("--advertise-name".to_string());
        args.push(req.name);
        args.push("--advertise-max-players".to_string());
        args.push(req.max_players.to_string());
    }
    spawn_detached_terminal(&exe, &args)
}

fn kill_local_port_owners(port: u16) {
    for pid in local_port_pids(port) {
        kill_pid(pid);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn local_port_pids(port: u16) -> Vec<u32> {
    let Ok(output) = std::process::Command::new("lsof")
        .args(["-t", &format!("-iUDP:{port}")])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

#[cfg(target_os = "windows")]
fn local_port_pids(port: u16) -> Vec<u32> {
    let Ok(output) = std::process::Command::new("netstat")
        .args(["-ano", "-p", "udp"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(&format!(":{port}")))
        .filter_map(|line| line.split_whitespace().last()?.parse().ok())
        .collect()
}

fn kill_pid(pid: u32) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_detached_terminal(exe: &std::path::Path, args: &[String]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(exe);
    command
        .args(args)
        .current_dir(workspace_root())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn().map(|_| ())
}

#[cfg(target_os = "windows")]
fn spawn_detached_terminal(exe: &std::path::Path, args: &[String]) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x00000010;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    std::process::Command::new("cmd.exe")
        .arg("/K")
        .arg(exe)
        .args(args)
        .current_dir(workspace_root())
        .creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}
