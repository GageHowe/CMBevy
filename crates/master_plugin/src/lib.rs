// this plugin is for organizing the plugins used by both server and client.
// we can do timing-dependent stuff here thanks to .before() etc
// this will get compiled twice due the disabled feature unification

pub use asset_pak::{register_asset_pak, register_asset_pak_with};
use bevy::prelude::*;
use common::{NetworkIDResource, game_state::*, slow_update::SlowSchedulePlugin, tick::*};
use gameplay::{
    GameplayPlugin,
    collision::CollisionPlugin,
    components::{atmosphere::AtmospherePlugin, gravity::GravityPlugin, snap::SnapPlugin},
    health::HealthPlugin,
    level::LevelPlugin,
    net::quic::NetPlugin,
    pawn::HeldWeaponMap,
    scripting::ScriptingPlugin,
};
use physics::physics_world::*;

pub struct MasterPlugin;
impl Plugin for MasterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(common::config::FIXED_TICK_RATE));
        app.insert_resource(Ticker { tick: 0 });
        app.init_state::<SimState>();

        // core rapier plugin
        app.add_plugins(PhysicsPlugin);
        app.add_plugins(GravityPlugin);
        app.add_plugins(SnapPlugin);

        // might delete this unless it's just the shader, then i'd move it to client
        app.add_plugins(AtmospherePlugin);

        // lua scripting
        app.add_plugins(ScriptingPlugin);

        // game-specific logic
        app.add_plugins(GameplayPlugin);

        // collision detection and dispatch
        app.add_plugins(CollisionPlugin);

        // health component and systems
        app.add_plugins(HealthPlugin);

        // deprecated/changed very soon
        app.add_plugins(LevelPlugin);

        app.init_resource::<HeldWeaponMap>();

        // tick increments AFTER FixedUpdate
        app.add_systems(FixedLast, increment_tick.in_set(SimulationSystems));

        app.add_plugins(NetPlugin);
        app.init_resource::<NetworkIDResource>();

        app.add_plugins(SlowSchedulePlugin);
        app.configure_sets(
            FixedPreUpdate,
            SimulationSystems.run_if(in_state(SimState::Playing)),
        )
        .configure_sets(
            FixedUpdate,
            SimulationSystems.run_if(in_state(SimState::Playing)),
        )
        .configure_sets(
            FixedPostUpdate,
            SimulationSystems.run_if(in_state(SimState::Playing)),
        )
        .configure_sets(
            PostUpdate,
            SimulationSystems.run_if(in_state(SimState::Playing)),
        )
        .configure_sets(
            FixedLast,
            SimulationSystems.run_if(in_state(SimState::Playing)),
        )
        .configure_sets(
            common::slow_update::SlowUpdate,
            SimulationSystems.run_if(in_state(SimState::Playing)),
        )
        .configure_sets(
            common::slow_update::SemiSlowUpdate,
            SimulationSystems.run_if(in_state(SimState::Playing)),
        );
        #[cfg(feature = "client")]
        app.configure_sets(
            Update,
            SimulationSystems.run_if(in_state(SimState::Playing)),
        );
    }
}
