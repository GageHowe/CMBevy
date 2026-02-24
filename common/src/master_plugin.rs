// this is for plugins and systems needed by both server and client
// only add systems that aren't timing-dependant

use bevy::prelude::*;
use crate::net::quic::QuicPlugin;
use crate::net::runtime::TokioRuntimePlugin;
use crate::physics::physics_world::*;
use crate::tick::*;

pub struct MasterPlugin;
impl Plugin for MasterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(64.0));
        app.add_plugins(TickPlugin);
        app.add_plugins(TokioRuntimePlugin);
        app.add_plugins(QuicPlugin);
        app.add_plugins(PhysicsPlugin); // always executes on FixedUpdate
    }
}