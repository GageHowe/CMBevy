use crate::resources::*;
use bevy::app::AppExit;
use bevy::prelude::*;
use net::quic::QuicManager;
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;

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
    use std::net::UdpSocket;
    use std::time::Duration;
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
    hosted: &mut HostedServer,
    port: u16,
    map: &str,
    gametype: &str,
    advertise: Option<http_common::RegisterRequest>,
) -> std::io::Result<()> {
    let mut child = spawn_gameserver(port, map, gametype)?;
    hosted.stdin = child.stdin.take().map(std::io::BufWriter::new);
    hosted.child = Some(child);
    if let Some(req) = advertise {
        beacon_register(req, std::sync::Arc::clone(&hosted.beacon_id));
    }
    Ok(())
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
    hosted: &mut HostedServer,
) {
    if let Some(quic) = quic {
        quic.disconnect();
        quic.inbound.clear();
    }
    if let Some(pending) = pending {
        pending.0 = None;
    }
    hosted.stdin = None;
    if let Some(mut child) = hosted.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Some(id) = hosted.beacon_id.lock().unwrap().take() {
        std::thread::spawn(move || {
            let _ = ureq::delete(&format!("{}/lobbies/{id}", common::config::BEACON_URL)).call();
        });
    }
}

fn beacon_register(
    req: http_common::RegisterRequest,
    id_slot: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) {
    std::thread::spawn(move || {
        if let Ok(resp) =
            ureq::post(&format!("{}/lobbies/register", common::config::BEACON_URL)).send_json(&req)
        {
            if let Ok(r) = resp.into_json::<http_common::RegisterResponse>() {
                *id_slot.lock().unwrap() = Some(r.id);
            }
        }
    });
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
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or(std::path::Path::new("."));
    dir.join(if cfg!(windows) {
        "gameserver.exe"
    } else {
        "gameserver"
    })
}

fn spawn_gameserver(port: u16, map: &str, gametype: &str) -> std::io::Result<std::process::Child> {
    let port = port.to_string();
    let mut command = std::process::Command::new(gameserver_exe());
    command
        .args(["--port", &port, "--map", map, "--gametype", gametype])
        .stdin(std::process::Stdio::piped());
    #[cfg(target_os = "linux")]
    unsafe {
        command.pre_exec(|| linux::set_parent_death_signal());
    }
    command.spawn()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io;

    const PR_SET_PDEATHSIG: i32 = 1;
    const SIGTERM: i32 = 15;

    unsafe extern "C" {
        fn prctl(option: i32, arg2: i32, arg3: usize, arg4: usize, arg5: usize) -> i32;
        fn getppid() -> i32;
    }

    pub(super) fn set_parent_death_signal() -> io::Result<()> {
        unsafe {
            if prctl(PR_SET_PDEATHSIG, SIGTERM, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if getppid() == 1 {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
        }
        Ok(())
    }
}
