use bevy::{
    ecs::schedule::{ExecutorKind, ScheduleLabel},
    prelude::*,
};

const FREQUENCY: f64 = 1.0; // x times / sec

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SlowUpdate;

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

#[derive(Resource)]
struct SlowScheduleState {
    accumulator: std::time::Duration,
    timestep: std::time::Duration,
}

impl Default for SlowScheduleState {
    fn default() -> Self {
        Self {
            accumulator: std::time::Duration::ZERO,
            timestep: std::time::Duration::from_secs_f64(1.0 / FREQUENCY),
        }
    }
}

pub struct SlowSchedulePlugin;

impl Plugin for SlowSchedulePlugin {
    fn build(&self, app: &mut App) {
        let mut schedule = Schedule::new(SlowUpdate);
        schedule.set_executor_kind(ExecutorKind::SingleThreaded);
        app.add_schedule(schedule);

        app.init_resource::<SlowScheduleState>();
        app.add_systems(RunFixedMainLoop, run_slow_update);
    }
}
