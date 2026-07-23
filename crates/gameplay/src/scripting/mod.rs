//! Lightweight Lua-backed gametype scripting.
//!
//! This crate owns the Lua VM, the small gameplay API exposed to scripts, and the
//! Bevy plugin that keeps scripts loaded and ticking on both client and server.

mod api;
mod config;
mod plugin;
mod runtime;
mod tag_index;

pub use config::ScriptConfig;
pub use plugin::ScriptingPlugin;
pub use runtime::{call_script_fn, get_script_global};
pub use tag_index::ScriptTagIndex;
