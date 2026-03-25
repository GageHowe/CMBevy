// this plugin is for organizing the plugins used by both server and client.
// we can do timing-dependent stuff here thanks to .before() etc
// this will get compiled twice due the disaled feature unification in game_objects

use bevy::prelude::*;
use bevy_quinnet::{client::QuinnetClientPlugin, server::QuinnetServerPlugin};
use common::NetworkIDResource;
use common::slow_update::SlowSchedulePlugin;
use common::tick::*;
use game_objects::atmosphere::AtmospherePlugin;
use game_objects::health::HealthPlugin;
use game_objects::planet::PlanetPlugin;
use net::quic::QuicManager;
use physics::convex_hull_asset::ConvexHullPlugin;
use physics::physics_world::*;
use scripting::ScriptingPlugin;

pub struct MasterPlugin;
impl Plugin for MasterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(common::config::FIXED_TICK_RATE));
        app.insert_resource(Ticker { tick: 0 });
        // step executes on FixedUpdate
        app.add_plugins(PhysicsPlugin);
        app.add_plugins(ConvexHullPlugin);
        app.add_plugins(PlanetPlugin);
        app.add_plugins(AtmospherePlugin);
        app.add_plugins(ScriptingPlugin);
        app.add_plugins(HealthPlugin);

        // tick should increment after everything else in FixedUpdate
        app.add_systems(FixedLast, increment_tick);

        app.add_plugins(QuinnetServerPlugin::default())
            .add_plugins(QuinnetClientPlugin::default())
            .init_resource::<QuicManager>()
            .init_resource::<NetworkIDResource>();

        // flush_outbound and process_inbound_* must be registered by each binary individually.

        app.add_plugins(SlowSchedulePlugin);
    }
}
