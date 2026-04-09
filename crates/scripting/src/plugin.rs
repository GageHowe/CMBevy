//! Wires the scripting runtime into Bevy schedules and keeps the Lua VM hot-reloaded.

use crate::api::register_script_functions;
use crate::config::ScriptConfig;
use crate::runtime::{ScriptRuntime, call_script, compile_script};
use crate::tag_index::{ScriptTagIndex, sync_script_tags};
use bevy::prelude::*;
use mlua::prelude::Lua;

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send_resource(ScriptRuntime {
            lua: Lua::new(),
            loaded: false,
        })
        .init_resource::<ScriptTagIndex>()
        .add_systems(Startup, (load, register_script_functions).chain())
        .add_systems(PreUpdate, sync_script_tags)
        .add_systems(Update, eval_script_update)
        .add_systems(FixedUpdate, (reload_script, eval_script_fixed_update));
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
