use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use mlua::prelude::*;
use crate::game_objects::pawn::biped;
use crate::game_objects::health::Health;
use crate::game_objects::weapon::{rifle, shotgun};
use crate::net::message::{NetworkID, NetworkIDResource};
use crate::physics::physics_world::PhysicsWorld;

#[derive(Resource, Clone)]
pub struct RhaiScriptConfig {
    pub path: String,
    pub is_server: bool,
    /// Pre-loaded source (e.g. received from server). Takes priority over `path`.
    pub source: Option<String>,
}

struct ScriptRuntime {
    lua: Lua,
    loaded: bool,
}
unsafe impl Sync for ScriptRuntime {}

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_non_send_resource(ScriptRuntime { lua: Lua::new(), loaded: false })
            .add_systems(Startup, (load, register_script_functions).chain())
            .add_systems(Update, (reload_script, eval_script_update))
            .add_systems(FixedUpdate, eval_script_fixed_update);
    }
}

fn compile_script(config: &RhaiScriptConfig, runtime: &mut ScriptRuntime) {
    let src = if let Some(s) = &config.source {
        s.clone()
    } else {
        match std::fs::read_to_string(&config.path) {
            Ok(s) => s,
            Err(err) => { error!("Failed to read Lua script '{}': {err}", config.path); return; }
        }
    };
    let _ = runtime.lua.globals().set("IS_SERVER", config.is_server);
    match runtime.lua.load(&src).exec() {
        Ok(_) => { runtime.loaded = true; info!("Lua script loaded from '{}'", config.path); }
        Err(err) => { error!("Failed to load Lua script '{}': {err}", config.path); }
    }
}

fn load(config: Option<Res<RhaiScriptConfig>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let Some(config) = config else { return };
    compile_script(&config, &mut runtime);
}

/// Recompiles the script whenever `RhaiScriptConfig` is inserted or changed at runtime
/// (e.g. when the client receives the gametype script from the server).
fn reload_script(config: Option<Res<RhaiScriptConfig>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let Some(config) = config else { return };
    if !config.is_changed() { return; }
    compile_script(&config, &mut runtime);
}

/// # Safety
/// Closures may only be called while `lua.app_data::<*mut World>` is set (done in
/// `call_script`/`call_script_fn`). Lua is single-threaded and `ScriptRuntime` is non-send.
fn register_script_functions(world: &mut World) {
    let runtime = world.remove_non_send_resource::<ScriptRuntime>().unwrap();

    runtime.lua.globals().set("get_health", runtime.lua.create_function(|lua, entity_id: i64| {
        let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
        let entity = Entity::from_bits(entity_id as u64);
        Ok(world.get::<Health>(entity).map(|h| h.current as i32).unwrap_or(0))
    }).unwrap()).unwrap();

    runtime.lua.globals().set("set_health", runtime.lua.create_function(|lua, (entity_id, amount): (i64, i32)| {
        let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
        let entity = Entity::from_bits(entity_id as u64);
        if let Some(mut health) = world.get_mut::<Health>(entity) {
            health.current = amount as f32;
        }
        Ok(())
    }).unwrap()).unwrap();

    // spawn(name, x, y, z) → entity_id
    runtime.lua.globals().set("spawn", runtime.lua.create_function(|lua, (name, x, y, z): (String, f64, f64, f64)| {
        let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
        let transform = Transform::from_translation(Vec3::new(x as f32, y as f32, z as f32));
        let net_id = NetworkID(world.resource_mut::<NetworkIDResource>().get_next_free_id());
        let mut state: SystemState<(Commands, ResMut<PhysicsWorld>)> = SystemState::new(world);
        let (mut commands, mut physics) = state.get_mut(world);
        let entity = match name.as_str() {
            "biped"   => biped::spawn(transform, &mut commands, &mut physics),
            "rifle"   => rifle::spawn(transform, &mut commands, &mut physics),
            "shotgun" => shotgun::spawn(transform, &mut commands, &mut physics),
            other => { error!("spawn: unknown entity '{other}'"); return Ok(-1i64); }
        };
        commands.entity(entity).insert(net_id);
        state.apply(world);
        Ok(entity.to_bits() as i64)
    }).unwrap()).unwrap();

    // despawn(entity_id)
    runtime.lua.globals().set("despawn", runtime.lua.create_function(|lua, entity_id: i64| {
        let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
        let entity = Entity::from_bits(entity_id as u64);
        let mut state: SystemState<Commands> = SystemState::new(world);
        let mut commands = state.get_mut(world);
        commands.entity(entity).despawn();
        state.apply(world);
        Ok(())
    }).unwrap()).unwrap();

    world.insert_non_send_resource(runtime);
}

/// Call a named function in the loaded Lua script, returning `None` if the script isn't
/// loaded or the function doesn't exist. Requires exclusive world access.
pub fn call_script_fn<T: mlua::FromLuaMulti>(world: &mut World, fn_name: &str) -> Option<T> {
    let runtime = world.remove_non_send_resource::<ScriptRuntime>()?;
    if !runtime.loaded { world.insert_non_send_resource(runtime); return None; }
    runtime.lua.set_app_data(world as *mut World);
    let result = runtime.lua.globals().get::<LuaFunction>(fn_name)
        .and_then(|f| f.call::<T>(()))
        .ok();
    runtime.lua.remove_app_data::<*mut World>();
    world.insert_non_send_resource(runtime);
    result
}

fn call_script(world: &mut World, fn_name: &str) {
    let Some(is_server) = world.get_resource::<RhaiScriptConfig>().map(|c| c.is_server) else { return };
    let runtime = world.remove_non_send_resource::<ScriptRuntime>().unwrap();
    if !runtime.loaded { world.insert_non_send_resource(runtime); return; }
    let _ = runtime.lua.globals().set("IS_SERVER", is_server);
    runtime.lua.set_app_data(world as *mut World);
    if let Ok(func) = runtime.lua.globals().get::<LuaFunction>(fn_name) {
        if let Err(err) = func.call::<()>(()) {
            error!("Lua {fn_name} error: {err}");
        }
    }
    runtime.lua.remove_app_data::<*mut World>();
    world.insert_non_send_resource(runtime);
}

fn eval_script_update(world: &mut World) {
    call_script(world, "on_tick");
}

fn eval_script_fixed_update(world: &mut World) {
    call_script(world, "on_fixed_tick");
}
