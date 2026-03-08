pub mod functions;

use bevy::prelude::*;
use rhai::{Engine, AST, Scope};

#[derive(Resource, Clone)]
pub struct RhaiScriptConfig {
    pub path: String, // e.g. "assets/ctf.gametype"
}

/// non-Read resource that contains script info
struct ScriptRuntime {
    engine: Engine,
    ast: Option<AST>,
    is_server: bool,
}

pub struct ScriptingPlugin {
    pub is_server: bool,
}

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_non_send_resource(ScriptRuntime {
                engine: Engine::new(),
                ast: None,
                is_server: self.is_server,
            })
            .add_systems(Startup, load_and_compile_rhai_script)
            .add_systems(FixedUpdate, eval_script_fixed_update);
    }
}

fn load_and_compile_rhai_script(
    config: Option<Res<RhaiScriptConfig>>,
    mut runtime: NonSendMut<ScriptRuntime>,
) {
    let Some(config) = config else { return };
    let path = &config.path;
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) => {
            error!("Failed to read Rhai script '{}': {err}", path);
            return;
        }
    };
    match runtime.engine.compile(&src) {
        Ok(ast) => {
            runtime.ast = Some(ast);
            info!("Rhai script compiled from '{}'", path);
        }
        Err(err) => {
            error!("Failed to compile Rhai script '{}': {err}", path);
        }
    }
}

/// evaluate the code at on_tick in the current gametype.
/// Code in this function should be fast since it's on the main thread and runs every fixed update.
fn eval_script_fixed_update(
    time: Res<Time>,
    mut runtime: NonSendMut<ScriptRuntime>,
) {
    let dt = time.delta_secs();
    let mut scope = Scope::new();
    scope.push_constant("is_server", runtime.is_server);

    let ScriptRuntime { engine, ast, .. } = &mut *runtime;

    let ast = match ast {
        Some(ast) => ast,
        None => return,
    };

    if let Err(err) = engine.call_fn::<()>(&mut scope, ast, "on_tick", (dt,)) {
        error!("Rhai on_tick error: {err}");
    }
}
