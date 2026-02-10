#[cfg(feature = "client")]
fn is_server() -> bool {
    false
}

#[cfg(feature = "server")]
fn is_server() -> bool {
    true
}
