use bevy::{
    ecs::schedule::{ScheduleLabel, SingleThreadedExecutor},
    prelude::*,
};

use crate::config::{SEMI_SLOW_UPDATE_FREQUENCY, SLOW_UPDATE_FREQUENCY};

/// runs every sec
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SlowUpdate;

/// runs every 0.25 secs
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SemiSlowUpdate;

fn run_slow_update(world: &mut World) {
    let delta = world.resource::<Time<Virtual>>().delta();

    world.resource_scope(|world, mut state: Mut<SlowScheduleState>| {
        state.accumulator += delta;
        let timestep = state.timestep;

        while state.accumulator >= timestep {
            state.accumulator -= timestep;
            world.run_schedule(SlowUpdate);
        }
    });
}

fn run_semi_slow_update(world: &mut World) {
    let delta = world.resource::<Time<Virtual>>().delta();

    world.resource_scope(|world, mut state: Mut<SemiSlowScheduleState>| {
        state.accumulator += delta;
        let timestep = state.timestep;

        while state.accumulator >= timestep {
            state.accumulator -= timestep;
            world.run_schedule(SemiSlowUpdate);
        }
    });
}

#[derive(Resource)]
struct SlowScheduleState {
    accumulator: std::time::Duration,
    timestep: std::time::Duration,
}

impl Default for SlowScheduleState {
    fn default() -> Self {
        Self {
            accumulator: std::time::Duration::ZERO,
            timestep: std::time::Duration::from_secs_f64(1.0 / SLOW_UPDATE_FREQUENCY),
        }
    }
}

#[derive(Resource)]
struct SemiSlowScheduleState {
    accumulator: std::time::Duration,
    timestep: std::time::Duration,
}

impl Default for SemiSlowScheduleState {
    fn default() -> Self {
        Self {
            accumulator: std::time::Duration::ZERO,
            timestep: std::time::Duration::from_secs_f64(1.0 / SEMI_SLOW_UPDATE_FREQUENCY),
        }
    }
}

pub struct SlowSchedulePlugin;

impl Plugin for SlowSchedulePlugin {
    fn build(&self, app: &mut App) {
        let mut schedule = Schedule::new(SlowUpdate);
        schedule.set_executor(SingleThreadedExecutor::new());
        app.add_schedule(schedule);
        let mut semi_schedule = Schedule::new(SemiSlowUpdate);
        semi_schedule.set_executor(SingleThreadedExecutor::new());
        app.add_schedule(semi_schedule);

        app.init_resource::<SlowScheduleState>();
        app.init_resource::<SemiSlowScheduleState>();
        app.add_systems(RunFixedMainLoop, (run_slow_update, run_semi_slow_update));
    }
}
