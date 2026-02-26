// #[cfg(feature = "client")]
// fn is_server() -> bool {
//     false
// }

// #[cfg(feature = "server")]
// fn is_server() -> bool {
//     true
// }

pub fn is_server() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|s| s.to_owned()))
        .map(|name| name == "server")
        .unwrap_or(false)
}