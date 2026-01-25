// config.rs: compilation options shared between server and client

// these should be the same, except when using emulate_network.go
pub const SERVER_BIND_ADDRESS: &str = "127.0.0.1:42070";
pub const CLIENT_CONNECT_ADDRESS: &str = "127.0.0.1:42069";

// avoids packet fragmentation. We should ensure packets are
// compressed to below this byte count in most cases
pub const MAX_UDP_SIZE: usize = 1200;
