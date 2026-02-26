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
        app.insert_resource(Ticker {
            tick: 0
        });
        // step executes on FixedUpdate
        app.add_plugins(PhysicsPlugin);

        // tick should increment after everything else in FixedUpdate
        app.add_systems(FixedPostUpdate, increment_tick);

        app.add_plugins(TokioRuntimePlugin);
        app.add_plugins(QuicPlugin);
    }
}

