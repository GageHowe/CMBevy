//! Script source selection and load context shared by client and server.

use bevy::prelude::*;

#[derive(Resource, Clone)]
pub struct ScriptConfig {
    pub path: String,
    pub is_server: bool,
    /// Pre-loaded source (e.g. received from server). Takes priority over `path`.
    pub source: Option<String>,
}
