// this is for plugins and systems needed by both server and client
// we can do timing-dependent stuff here thanks to .before() etc

use bevy::prelude::*;
use bevy_quinnet::{client::QuinnetClientPlugin, server::QuinnetServerPlugin};
use crate::net::quic::{flush_outbound, process_inbound, QuicManager};
use crate::physics::physics_world::*;
use crate::tick::*;
use crate::assets::CMAssetPlugin;
use crate::net::message::NetworkIDResource;

/// Controls which quinnet plugin(s) are loaded.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    /// Load only the client plugin (for the game client binary).
    #[default]
    Client,
    /// Load only the server plugin (for the dedicated server binary).
    Server,
}

/// Plugin shared between the server and client binaries.
///
/// Use [`MasterPlugin::server()`] in the server binary and
/// [`MasterPlugin::client()`] (or `MasterPlugin::default()`) in the client binary
/// so only the relevant quinnet endpoint plugin is loaded.
pub struct MasterPlugin {
    pub mode: NetworkMode,
}

impl MasterPlugin {
    pub fn server() -> Self {
        Self { mode: NetworkMode::Server }
    }
    pub fn client() -> Self {
        Self { mode: NetworkMode::Client }
    }
}

impl Default for MasterPlugin {
    fn default() -> Self {
        Self::client()
    }
}

impl Plugin for MasterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.add_plugins(CMAssetPlugin);
        app.insert_resource(Ticker { tick: 0 });

        // step executes on FixedUpdate
        app.add_plugins(PhysicsPlugin);

        // tick should increment after everything else in FixedUpdate
        app.add_systems(FixedPostUpdate, increment_tick);

        // Only load the quinnet plugin(s) appropriate for this binary.
        match self.mode {
            NetworkMode::Server => { app.add_plugins(QuinnetServerPlugin::default()); }
            NetworkMode::Client => { app.add_plugins(QuinnetClientPlugin::default()); }
        }

        app.init_resource::<QuicManager>()
            .init_resource::<NetworkIDResource>()
            // process_inbound fills the inbound queue; flush_outbound drains the outbound queue.
            // Both run in Update. on_message (game logic) runs only in FixedUpdate so it
            // processes the accumulated queue at the simulation tick rate.
            .add_systems(Update, (process_inbound, flush_outbound).chain());
    }
}
