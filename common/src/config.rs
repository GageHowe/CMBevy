// config.rs: compilation options shared between server and client

// hard-coded for now, might stay, idk
pub const SERVER_BIND_ADDRESS: &str = "127.0.0.1:42070";

/// Fixed-update rate for both the Bevy schedule and the Rapier physics integrator
pub const TICK_RATE: f64 = 60.0;

/// Seconds after death before the server respawns the player. This may not be const in the future
pub const RESPAWN_DELAY_SECS: f32 = 5.0;

// avoids packet fragmentation. We should ensure packets are compressed to below this byte count in most cases
// currently unused
pub const MAX_UDP_SIZE: usize = 1200;
