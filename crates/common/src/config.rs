use std::path::{Path, PathBuf};

pub const SERVER_BIND_ADDRESS: &str = "127.0.0.1:42070";
/// UDP port the gameserver listens on for LAN discovery probes.
pub const LAN_DISCOVERY_PORT: u16 = 42071;
pub const BEACON_RENDEZVOUS_PORT: u16 = 42072;
pub const BEACON_URL: &str = "https://criticalmass.dev";
pub const FIXED_TICK_RATE: f64 = 60.0;
pub const RESPAWN_DELAY_SECS: f32 = 5.0;

// avoids packet fragmentation. We should ensure packets are compressed to below this byte count in most cases
// currently unused
pub const MAX_UDP_SIZE: usize = 1200;

pub fn executable_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn runtime_path(path: impl AsRef<Path>) -> PathBuf {
    executable_dir().join(path)
}

pub fn asset_dir() -> PathBuf {
    let cwd_assets = std::env::current_dir().ok().map(|dir| dir.join("assets"));
    if let Some(path) = cwd_assets
        && path.exists()
    {
        return path;
    }
    runtime_path("assets")
}

pub fn beacon_rendezvous_addr() -> String {
    let rest = BEACON_URL
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(BEACON_URL);
    let host = rest.split('/').next().unwrap_or(rest);
    let host = host.rsplit_once(':').map(|(host, _)| host).unwrap_or(host);
    format!("{host}:{BEACON_RENDEZVOUS_PORT}")
}
