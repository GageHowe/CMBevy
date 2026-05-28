// this plugin is for organizing the plugins used by both server and client.
// we can do timing-dependent stuff here thanks to .before() etc
// this will get compiled twice due the disabled feature unification

use bevy::prelude::*;
use common::{NetworkIDResource, slow_update::SlowSchedulePlugin, tick::*};
use gameplay::{
    GameplayPlugin,
    collision::CollisionPlugin,
    components::{atmosphere::AtmospherePlugin, gravity::GravityPlugin, snap::SnapPlugin},
    health::HealthPlugin,
    level::LevelPlugin,
    pawn::HeldWeaponMap,
};
#[cfg(feature = "client")]
use net::clientonly::NetClientPlugin;
#[cfg(not(feature = "client"))]
use net::serveronly::NetServerPlugin;
use physics::physics_world::*;
use scripting::ScriptingPlugin;

pub struct MasterPlugin;
impl Plugin for MasterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(common::config::FIXED_TICK_RATE));
        app.insert_resource(Ticker { tick: 0 });
        app.add_plugins(PhysicsPlugin);
        app.add_plugins(GravityPlugin);
        app.add_plugins(SnapPlugin);
        app.add_plugins(AtmospherePlugin);
        app.add_plugins(ScriptingPlugin);
        app.add_plugins(GameplayPlugin);
        app.add_plugins(CollisionPlugin);
        app.add_plugins(HealthPlugin);
        app.add_plugins(LevelPlugin);
        app.init_resource::<HeldWeaponMap>();

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
