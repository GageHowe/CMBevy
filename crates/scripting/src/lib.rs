use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use mlua::prelude::*;
// use game_objects::pawn::biped;
// use game_objects::weapon::{rifle, shotgun};
use game_objects::{GameObject, GenericShape, spawn_generic};
use game_objects::health::Health;
use common::{NetworkID, NetworkIDResource};
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::PhysicsWorld;
use rapier3d::prelude::ColliderBuilder;

#[derive(Resource, Clone)]
pub struct ScriptConfig {
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

fn compile_script(config: &ScriptConfig, runtime: &mut ScriptRuntime) {
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

fn load(config: Option<Res<ScriptConfig>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let Some(config) = config else { return };
    compile_script(&config, &mut runtime);
}

/// Recompiles the script whenever `ScriptConfig` is inserted or changed at runtime
/// (e.g. when the client receives the gametype script from the server).
fn reload_script(config: Option<Res<ScriptConfig>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let Some(config) = config else { return };
    if !config.is_changed() { return; }
    compile_script(&config, &mut runtime);
}

/// # Safety
/// Closures may only be called while `lua.app_data::<*mut World>` is set (done in
/// `call_script`/`call_script_fn`). Lua is single-threaded and `ScriptRuntime` is non-send.
fn register_script_functions(world: &mut World) {
    let runtime = world.remove_non_send_resource::<ScriptRuntime>().unwrap();

    // gets the current health of the specified entity ID
    runtime.lua.globals().set("get_health", runtime.lua.create_function(|lua, entity_id: i64| {
        let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
        let entity = Entity::from_bits(entity_id as u64);
        Ok(world.get::<Health>(entity).map(|h| h.current as i32).unwrap_or(0))
    }).unwrap()).unwrap();

    // sets the current health of the specified entity ID
    runtime.lua.globals().set("set_health", runtime.lua.create_function(|lua, (entity_id, amount): (i64, i32)| {
        let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
        let entity = Entity::from_bits(entity_id as u64);
        if let Some(mut health) = world.get_mut::<Health>(entity) {
            health.current = amount as f32;
        }
        Ok(())
    }).unwrap()).unwrap();

    // this isn't ready yet
    // // spawn(name, x, y, z) → entity_id
    // runtime.lua.globals().set("spawn", runtime.lua.create_function(|lua, (name, x, y, z): (String, f64, f64, f64)| {
    //     let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
    //     let transform = Transform::from_translation(Vec3::new(x as f32, y as f32, z as f32));
    //     let net_id = NetworkID(world.resource_mut::<NetworkIDResource>().next());
    //     let mut state: SystemState<(Commands, ResMut<PhysicsWorld>)> = SystemState::new(world);
    //     let (mut commands, mut physics) = state.get_mut(world);
    //     let entity = match name.as_str() {
    //         "biped"   => biped::BipedPawnComponent::spawn_physics(transform, &mut commands, &mut physics),
    //         "rifle"   => rifle::RifleComponent::spawn_physics(transform, &mut commands, &mut physics),
    //         "shotgun" => shotgun::ShotgunComponent::spawn_physics(transform, &mut commands, &mut physics),
    //         other => { error!("spawn: unknown entity '{other}'"); return Ok(-1i64); }
    //     };
    //     commands.entity(entity).insert(net_id);
    //     state.apply(world);
    //     Ok(entity.to_bits() as i64)
    // }).unwrap()).unwrap();

    // spawn_box(x, y, z, hx, hy, hz, friction, restitution) → entity_id
    runtime.lua.globals().set("spawn_box", runtime.lua.create_function(|lua, (x, y, z, hx, hy, hz, friction, restitution): (f64, f64, f64, f64, f64, f64, f64, f64)| {
        let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
        let transform = Transform::from_translation(Vec3::new(x as f32, y as f32, z as f32));
        let net_id = NetworkID(world.resource_mut::<NetworkIDResource>().next());
        let mut state: SystemState<(Commands, ResMut<PhysicsWorld>)> = SystemState::new(world);
        let (mut commands, mut physics) = state.get_mut(world);
        let shape = GenericShape::Primitive(ColliderBuilder::cuboid(hx as f32, hy as f32, hz as f32).friction(friction as f32).restitution(restitution as f32));
        let entity = spawn_generic(transform, shape, None, Some(net_id), &mut commands, &mut physics);
        state.apply(world);
        Ok(entity.to_bits() as i64)
    }).unwrap()).unwrap();

    // spawn_hull(x, y, z, hull_path, scale, friction, restitution, mesh_path) → entity_id
    // mesh_path is optional (nil to skip visuals)
    // e.g. local e = spawn_hull(0, 0, 0, "collision/rock.obj", 5.0, 0.8, 0.2, "models/rock.glb#Scene0")
    runtime.lua.globals().set("spawn_hull", runtime.lua.create_function(|lua, (x, y, z, path, scale, friction, restitution, mesh_path): (f64, f64, f64, String, f64, f64, f64, Option<String>)| {
        let world = unsafe { &mut **lua.app_data_ref::<*mut World>().unwrap() };
        let transform = Transform::from_translation(Vec3::new(x as f32, y as f32, z as f32));
        let net_id = NetworkID(world.resource_mut::<NetworkIDResource>().next());
        let mut state: SystemState<(Commands, ResMut<PhysicsWorld>, Option<Res<AssetServer>>, Option<Res<Assets<ConvexHullAsset>>>)> = SystemState::new(world);
        let (mut commands, mut physics, asset_server, hull_assets) = state.get_mut(world);
        let (Some(asset_server), Some(hull_assets)) = (asset_server.as_deref(), hull_assets.as_deref()) else {
            error!("spawn_hull: asset pipeline unavailable");
            return Ok(-1i64);
        };
        let leaked: &'static str = Box::leak(path.into_boxed_str());
        let shape = GenericShape::Hull { path: leaked, scale: scale as f32, asset_server, hull_assets };
        let entity = spawn_generic(transform, shape, None, Some(net_id), &mut commands, &mut physics);
        if let Some(mesh_path) = mesh_path {
            commands.entity(entity).insert((SceneRoot(asset_server.load(mesh_path)), Visibility::default()));
        }
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

/// Read a named global from the loaded Lua script. Returns `None` if the script isn't
/// loaded or the global doesn't exist / has the wrong type.
pub fn get_script_global<T: FromLua>(world: &mut World, name: &str) -> Option<T> {
    let runtime = world.remove_non_send_resource::<ScriptRuntime>()?;
    if !runtime.loaded { world.insert_non_send_resource(runtime); return None; }
    let result = runtime.lua.globals().get::<T>(name).ok();
    world.insert_non_send_resource(runtime);
    result
}

/// Call a named function in the loaded Lua script, returning `None` if the script isn't
/// loaded or the function doesn't exist. Requires exclusive world access.
pub fn call_script_fn<T: FromLuaMulti>(world: &mut World, fn_name: &str) -> Option<T> {
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
    let Some(is_server) = world.get_resource::<ScriptConfig>().map(|c| c.is_server) else { return };
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
