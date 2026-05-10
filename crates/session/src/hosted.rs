use bevy::{app::AppExit, prelude::*};
use net::quic::QuicManager;

use crate::resources::*;

pub fn available_maps() -> Vec<String> {
    scan_dir(common::config::asset_dir().join("maps"), "ron")
}

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

/// Starts a hosted dedicated server locally by clearing the requested UDP port,
/// launching the server in a detached terminal, and waiting for it to bind.
pub fn start_hosted_server(
    _hosted: &mut HostedServer,
    port: u16,
    map: &str,
    gametype: &str,
    advertise: Option<http_common::RegisterRequest>,
) -> std::io::Result<()> {
    let preflight = match std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port)) {
        Ok(sock) => sock,
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
            kill_local_port_owners(port);
            std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port))?
        }
        Err(err) => return Err(err),
    };
    drop(preflight);
    spawn_gameserver_terminal(port, map, gametype, advertise)?;
    wait_for_port_bind(port)
}

pub fn cleanup_before_app_exit(
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

pub fn exit_after_returning_to_menu(
    pending_exit: Res<PendingExit>,
    mut exit: MessageWriter<AppExit>,
) {
    if pending_exit.0 {
        exit.write(AppExit::Success);
    }
}

pub fn shutdown_session(
    quic: Option<&mut QuicManager>,
    pending: Option<&mut PendingReconciliation>,
    _hosted: &mut HostedServer,
) {
    if let Some(quic) = quic {
        quic.disconnect();
        quic.inbound.clear();
    }
    if let Some(pending) = pending {
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

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_root) = manifest_dir.parent().and_then(|dir| dir.parent()) {
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

fn workspace_root() -> Option<std::path::PathBuf> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|dir| dir.parent())
        .map(std::path::Path::to_path_buf)
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

fn wait_for_port_bind(port: u16) -> std::io::Result<()> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port)) {
            Ok(sock) => {
                drop(sock);
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => return Ok(()),
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "gameserver did not bind its port",
    ))
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
        .filter_map(|line| line.trim().parse::<u32>().ok())
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
        .filter_map(|line| {
            let cols: Vec<_> = line.split_whitespace().collect();
            if cols.len() < 4 {
                return None;
            }
            let local = cols[1];
            let pid = cols[3];
            local
                .rsplit(':')
                .next()
                .filter(|p| *p == port.to_string())
                .and_then(|_| pid.parse::<u32>().ok())
        })
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn local_port_pids(_port: u16) -> Vec<u32> {
    Vec::new()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
}

#[cfg(target_os = "windows")]
fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn kill_pid(_pid: u32) {}

#[cfg(target_os = "linux")]
// Linux needs an actual terminal launcher here. `xdg-open` follows file associations and may
// open editors instead of terminals, so use `xdg-terminal-exec` for the default terminal path.
fn spawn_detached_terminal(exe: &std::path::Path, args: &[String]) -> std::io::Result<()> {
    let cwd = workspace_root().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    std::process::Command::new("xdg-terminal-exec")
        .current_dir(cwd)
        .arg(exe)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "xdg-terminal-exec not found; install it to launch the system default terminal",
                )
            } else {
                err
            }
        })
}

#[cfg(target_os = "windows")]
fn spawn_detached_terminal(exe: &std::path::Path, args: &[String]) -> std::io::Result<()> {
    let mut command = std::process::Command::new("cmd");
    command.arg("/C").arg("start").arg("Hosted Server").arg(exe);
    command.args(args);
    command.spawn().map(|_| ())
}

#[cfg(target_os = "macos")]
fn spawn_detached_terminal(exe: &std::path::Path, args: &[String]) -> std::io::Result<()> {
    let script = format!(
        "tell application \"Terminal\" to do script {}",
        apple_script_string(&shell_command_line(exe, args))
    );
    std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .spawn()
        .map(|_| ())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn spawn_detached_terminal(_exe: &std::path::Path, _args: &[String]) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "hosted server terminal launch is unsupported on this platform",
    ))
}

#[cfg(target_os = "macos")]
fn shell_command_line(exe: &std::path::Path, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(shell_quote(&exe.to_string_lossy()));
    parts.extend(args.iter().map(|arg| shell_quote(arg)));
    parts.join(" ")
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn apple_script_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
