// config.rs: compilation options shared between server and client

// these should be the same, except when using emulate_network.go
pub const SERVER_BIND_ADDRESS: &str = "127.0.0.1:42070";

/// Fixed-update rate for both the Bevy schedule and the Rapier physics integrator.
/// Must match in both places — use `config::TICK_RATE` everywhere instead of a literal.
pub const TICK_RATE: f64 = 64.0;
// pub const CLIENT_CONNECT_ADDRESS: &str = "127.0.0.1:42069"; // currently unused

// avoids packet fragmentation. We should ensure packets are compressed to below this byte count in most cases
// currently unused
pub const MAX_UDP_SIZE: usize = 1200;
