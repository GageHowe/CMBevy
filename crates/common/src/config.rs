pub const SERVER_BIND_ADDRESS: &str = "127.0.0.1:42070";
/// UDP port the gameserver listens on for LAN discovery probes.
pub const LAN_DISCOVERY_PORT: u16 = 42071;
pub const BEACON_URL: &str = "https://cmbevy.onrender.com";
pub const FIXED_TICK_RATE: f64 = 60.0;
pub const RESPAWN_DELAY_SECS: f32 = 5.0;

// avoids packet fragmentation. We should ensure packets are compressed to below this byte count in most cases
// currently unused
pub const MAX_UDP_SIZE: usize = 1200;
