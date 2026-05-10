//! Owns the Lua VM lifecycle and the small helpers used to call into it safely.

use bevy::prelude::*;
use mlua::prelude::*;

use crate::config::ScriptConfig;

pub(crate) struct ScriptRuntime {
    pub lua: Lua,
    pub loaded: bool,
}
unsafe impl Sync for ScriptRuntime {}

pub(crate) fn compile_script(config: &ScriptConfig, runtime: &mut ScriptRuntime) {
    let src = if let Some(s) = &config.source {
        s.clone()
    } else {
        match std::fs::read_to_string(&config.path) {
            Ok(s) => s,
            Err(err) => {
                error!("Failed to read Lua script '{}': {err}", config.path);
                return;
            }
        }
    };
    let _ = runtime.lua.globals().set("IS_SERVER", config.is_server);
    let _ = runtime
        .lua
        .globals()
        .set("FIXED_TICK_RATE", common::config::FIXED_TICK_RATE);
    let _ = runtime
        .lua
        .globals()
        .set("FIXED_DELTA_SECONDS", 1.0 / common::config::FIXED_TICK_RATE);
    match runtime.lua.load(&src).exec() {
        Ok(_) => {
            runtime.loaded = true;
            info!("Lua script loaded from '{}'", config.path);
        }
        Err(err) => {
            error!("Failed to load Lua script '{}': {err}", config.path);
        }
    }
}

pub fn get_script_global<T: FromLua>(world: &mut World, name: &str) -> Option<T> {
    let runtime = world.remove_non_send_resource::<ScriptRuntime>()?;
    if !runtime.loaded {
        world.insert_non_send_resource(runtime);
        return None;
    }
    let result = runtime.lua.globals().get::<T>(name).ok();
    world.insert_non_send_resource(runtime);
    result
}

pub fn call_script_fn<T: FromLuaMulti>(world: &mut World, fn_name: &str) -> Option<T> {
    let runtime = world.remove_non_send_resource::<ScriptRuntime>()?;
    if !runtime.loaded {
        world.insert_non_send_resource(runtime);
        return None;
    }
    runtime.lua.set_app_data(world as *mut World);
    let result = runtime
        .lua
        .globals()
        .get::<LuaFunction>(fn_name)
        .and_then(|f| f.call::<T>(()))
        .ok();
    runtime.lua.remove_app_data::<*mut World>();
    world.insert_non_send_resource(runtime);
    result
}

pub(crate) fn call_script_args<A: IntoLuaMulti>(world: &mut World, fn_name: &str, args: A) {
    let Some(is_server) = world.get_resource::<ScriptConfig>().map(|c| c.is_server) else {
        return;
    };
    let Some(runtime) = world.remove_non_send_resource::<ScriptRuntime>() else {
        return;
    };
    if !runtime.loaded {
        world.insert_non_send_resource(runtime);
        return;
    }
    let _ = runtime.lua.globals().set("IS_SERVER", is_server);
    runtime.lua.set_app_data(world as *mut World);
    if let Ok(func) = runtime.lua.globals().get::<LuaFunction>(fn_name)
        && let Err(err) = func.call::<()>(args)
    {
        error!("Lua {fn_name} error: {err}");
    }
    runtime.lua.remove_app_data::<*mut World>();
    world.insert_non_send_resource(runtime);
}

pub(crate) fn call_script(world: &mut World, fn_name: &str) {
    call_script_args(world, fn_name, ());
}
