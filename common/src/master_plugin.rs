// this is for plugins and systems needed by both server and client
// we can do timing-dependent stuff here thanks to .before() etc

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
        app.add_systems(FixedUpdate, increment_tick.before(step_physics)); // should execute before physics
        app.add_plugins(PhysicsPlugin); // internal system always executes on FixedUpdate

        app.add_plugins(TokioRuntimePlugin);
        app.add_plugins(QuicPlugin);
    }
}

