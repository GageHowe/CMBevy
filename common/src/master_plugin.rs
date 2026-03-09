// this is for plugins and systems needed by both server and client
// we can do timing-dependent stuff here thanks to .before() etc

use bevy::prelude::*;
use bevy_quinnet::{client::QuinnetClientPlugin, server::QuinnetServerPlugin};
use crate::net::quic::{flush_outbound, QuicManager};
use crate::physics::physics_world::*;
use crate::tick::*;
use crate::net::message::NetworkIDResource;
use crate::slow_update::SlowSchedulePlugin;

pub struct MasterPlugin;
impl Plugin for MasterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(crate::config::FIXED_TICK_RATE));
        app.insert_resource(Ticker {
            tick: 0
        });
        // step executes on FixedUpdate
        app.add_plugins(PhysicsPlugin);

        // tick should increment after everything else in FixedUpdate
        app.add_systems(FixedLast, increment_tick);

        app.add_plugins(QuinnetServerPlugin::default())
            .add_plugins(QuinnetClientPlugin::default())
            .init_resource::<QuicManager>()
            .init_resource::<NetworkIDResource>()
            // flush_outbound in PostUpdate so all FixedUpdate and Update sends are flushed together.
            // process_inbound_server / process_inbound_client must be registered by each binary
            // individually in PreUpdate, so each binary only runs the relevant path.
            .add_systems(PostUpdate, flush_outbound);

        app.add_plugins(SlowSchedulePlugin);
    }
}
