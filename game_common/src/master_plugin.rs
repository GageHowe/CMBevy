// this is for plugins and systems needed by both server and client
// we can do timing-dependent stuff here thanks to .before() etc

use bevy::prelude::*;
use bevy_quinnet::{client::QuinnetClientPlugin, server::QuinnetServerPlugin};
use crate::net::quic::{flush_outbound, process_inbound, QuicManager};
use crate::physics::physics_world::*;
use crate::tick::*;
use crate::net::message::NetworkIDResource;

pub struct MasterPlugin;
impl Plugin for MasterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.insert_resource(Ticker {
            tick: 0
        });
        // step executes on FixedUpdate
        app.add_plugins(PhysicsPlugin);

        // tick should increment after everything else in FixedUpdate
        app.add_systems(FixedPostUpdate, increment_tick);

        app.add_plugins(QuinnetServerPlugin::default())
            .add_plugins(QuinnetClientPlugin::default())
            .init_resource::<QuicManager>()
            .init_resource::<NetworkIDResource>()
            // process_inbound in PreUpdate so the queue is filled before FixedUpdate systems run.
            // flush_outbound in PostUpdate so all FixedUpdate and Update sends are flushed together.
            .add_systems(PreUpdate, process_inbound)
            .add_systems(PostUpdate, flush_outbound);
    }
}
