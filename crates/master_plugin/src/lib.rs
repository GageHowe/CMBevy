// this plugin is for organizing the plugins used by both server and client.
// we can do timing-dependent stuff here thanks to .before() etc
// this will get compiled twice due the disaled feature unification in game_objects

use bevy::prelude::*;
use common::NetworkIDResource;
use common::slow_update::SlowSchedulePlugin;
use common::tick::*;
use game_objects::components::atmosphere::AtmospherePlugin;
use game_objects::generic::swap_hull_colliders;
use game_objects::health::HealthPlugin;
use game_objects::components::planet::PlanetPlugin;
#[cfg(feature = "client")]
use net::clientonly::NetClientPlugin;
#[cfg(not(feature = "client"))]
use net::serveronly::NetServerPlugin;
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
        app.add_systems(FixedUpdate, swap_hull_colliders);

        // tick should increment after everything else in FixedUpdate
        app.add_systems(FixedLast, increment_tick);

        #[cfg(not(feature = "client"))]
        app.add_plugins(NetServerPlugin);
        #[cfg(feature = "client")]
        app.add_plugins(NetClientPlugin);
        app.init_resource::<NetworkIDResource>();

        app.add_plugins(SlowSchedulePlugin);
    }
}
