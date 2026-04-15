//! Wires the scripting runtime into Bevy schedules and keeps the Lua VM hot-reloaded.

use bevy::prelude::*;
use game_objects::{
    health::{PendingPlayerKills, PendingPlayerRemovals, handle_deaths},
    pawn::PlayerRegistry,
};
use mlua::prelude::Lua;

use crate::{
    api::register_script_functions,
    config::ScriptConfig,
    runtime::{ScriptRuntime, call_script, call_script_args, compile_script},
    tag_index::{ScriptTagIndex, sync_script_tags},
};

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send_resource(ScriptRuntime { lua: Lua::new(), loaded: false })
            .init_resource::<ScriptTagIndex>()
            .init_resource::<PendingPlayerKills>()
            .init_resource::<PendingPlayerRemovals>()
            .add_systems(Startup, (load, register_script_functions).chain())
            .add_systems(PreUpdate, sync_script_tags)
            .add_systems(Update, eval_script_update)
            .add_systems(FixedUpdate, (reload_script, eval_script_fixed_update))
            .add_systems(FixedUpdate, dispatch_player_kill_callbacks.after(handle_deaths));
    }
}

fn load(config: Option<Res<ScriptConfig>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let Some(config) = config else { return };
    compile_script(&config, &mut runtime);
}

/// Recompiles the script whenever `ScriptConfig` is inserted or changed at runtime
/// (e.g. when the client receives the gametype script from the server).
fn reload_script(config: Option<Res<ScriptConfig>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let Some(config) = config else { return };
    if !config.is_changed() {
        return;
    }
    compile_script(&config, &mut runtime);
}

fn eval_script_update(world: &mut World) {
    call_script(world, "on_tick");
}

fn eval_script_fixed_update(world: &mut World) {
    call_script(world, "on_fixed_tick");
}

/// Drains deferred kill callbacks after authoritative death handling so scripts can award
/// numbers or end the game without Rust hard-coding scoring rules.
fn dispatch_player_kill_callbacks(world: &mut World) {
    let kills = world
        .get_resource_mut::<PendingPlayerKills>()
        .map(|mut pending| std::mem::take(&mut pending.0))
        .unwrap_or_default();
    for (victim, killer) in kills {
        call_script_args(
            world,
            "on_player_killed",
            (victim.to_bits() as i64, killer.map(|entity| entity.to_bits() as i64)),
        );
    }

    let removals = world
        .get_resource_mut::<PendingPlayerRemovals>()
        .map(|mut pending| std::mem::take(&mut pending.0))
        .unwrap_or_default();
    if removals.is_empty() {
        return;
    }
    let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() else {
        return;
    };
    for entity in removals {
        let _ = registry.remove_character(entity);
    }
}
