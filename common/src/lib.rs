#![feature(likely_unlikely)]

pub mod config;
pub mod macros;
pub mod level;
pub mod net;
pub mod game_objects;
pub use game_objects::health;
pub use game_objects::pawn;
pub use game_objects::weapon;
pub mod physics;
pub mod ring_buffer;
pub mod types;
pub mod tick;
pub mod master_plugin;
pub mod slow_update;
pub mod interaction;
pub mod scripting;

// client-only modules
#[cfg(feature = "client")]
pub mod steam;
#[cfg(feature = "client")]
pub mod settings;
#[cfg(feature = "client")]
pub mod ui;
#[cfg(feature = "client")]
pub mod camera;