#[cfg(feature = "client")]
pub mod clientonly;
pub mod message;
pub mod quic;
pub mod replication;
#[cfg(not(feature = "client"))]
pub mod serveronly;

pub fn format_packet_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f32 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f32 / (1024.0 * 1024.0))
    }
}
