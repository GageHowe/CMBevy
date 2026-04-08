use std::path::{Path, PathBuf};

pub const SERVER_BIND_ADDRESS: &str = "127.0.0.1:42070";
/// UDP port the gameserver listens on for LAN discovery probes.
pub const LAN_DISCOVERY_PORT: u16 = 42071;
pub const BEACON_URL: &str = "https://cmbevy.onrender.com";
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
    runtime_path("assets")
}
