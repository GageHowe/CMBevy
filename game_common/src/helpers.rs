// simple helpers

pub fn is_server() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|s| s.to_owned()))
        .map(|name| name == "server")
        .unwrap_or(false)
}

/// use with `use common::debug_println;`
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            println!($($arg)*);
        }
    };
}