use bevy::prelude::*;
use rhai::{Engine, AST, Scope};
use std::cell::Cell;

#[derive(Resource, Clone)]
pub struct RhaiScriptConfig {
    pub path: String,
    pub is_server: bool,
}

struct ScriptRuntime {
    engine: Engine,
    ast: Option<AST>,
    world_ptr: Cell<*mut World>,
}
unsafe impl Sync for ScriptRuntime {}

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_non_send_resource(ScriptRuntime {
                engine: Engine::new(),
                ast: None,
                world_ptr: Cell::new(std::ptr::null_mut()),
            })
            .add_systems(Startup, (load, register_script_functions).chain())
            .add_systems(FixedUpdate, eval_script_fixed_update);
    }
}

fn load(
    config: Option<Res<RhaiScriptConfig>>,
    mut runtime: NonSendMut<ScriptRuntime>,
) {
    let Some(config) = config else { return };
    let src = match std::fs::read_to_string(&config.path) {
        Ok(s) => s,
        Err(err) => {
            error!("Failed to read Rhai script '{}': {err}", config.path);
            return;
        }
    };
    match runtime.engine.compile(&src) {
        Ok(ast) => {
            runtime.ast = Some(ast);
            info!("Rhai script compiled from '{}'", config.path);
        }
        Err(err) => {
            error!("Failed to compile Rhai script '{}': {err}", config.path);
        }
    }
}

fn register_script_functions(world: &mut World) {
    let mut runtime = world.remove_non_send_resource::<ScriptRuntime>().unwrap();
    let world_ptr = runtime.world_ptr.as_ptr() as *const Cell<*mut World>;

    runtime.engine.register_fn("get_health", move |entity_id: i64| -> i32 {
        let world = unsafe {
            let ptr = (*world_ptr).get();
            assert!(!ptr.is_null(), "world_ptr not set");
            &mut *ptr
        };
        let entity = Entity::from_bits(entity_id as u64);
        world.get::<Health>(entity).map(|h| h.0).unwrap_or(0)
    });

    runtime.engine.register_fn("set_health", move |entity_id: i64, amount: i32| {
        let world = unsafe {
            let ptr = (*world_ptr).get();
            assert!(!ptr.is_null(), "world_ptr not set");
            &mut *ptr
        };
        let entity = Entity::from_bits(entity_id as u64);
        if let Some(mut health) = world.get_mut::<Health>(entity) {
            health.0 = amount;
        }
    });

    world.insert_non_send_resource(runtime);
}

fn eval_script_fixed_update(world: &mut World) {
    let dt = world.resource::<Time>().delta_secs();
    let is_server = world
        .get_resource::<RhaiScriptConfig>()
        .map(|c| c.is_server)
        .unwrap_or(false);

    let runtime = world.remove_non_send_resource::<ScriptRuntime>().unwrap();

    let Some(ast) = runtime.ast.clone() else {
        world.insert_non_send_resource(runtime);
        return;
    };

    runtime.world_ptr.set(world as *mut World);

    let mut scope = Scope::new();
    scope.push_constant("is_server", is_server);

    if let Err(err) = runtime.engine.call_fn::<()>(&mut scope, &ast, "on_tick", (dt,)) {
        error!("Rhai on_tick error: {err}");
    }

    runtime.world_ptr.set(std::ptr::null_mut());
    world.insert_non_send_resource(runtime);
}

// Replace with your actual component
#[derive(Component)]
struct Health(i32);