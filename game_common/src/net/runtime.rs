// defines an async tokio runtime to run alongside bevy and handle networking/quic

use bevy::prelude::*;

// use bevy::prelude::*;
use tokio::runtime::Runtime;

/// Plugin that adds a Tokio runtime to Bevy as a resource
pub struct TokioRuntimePlugin;
impl Plugin for TokioRuntimePlugin {
    fn build(&self, app: &mut App) {
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");
        app.insert_resource(TokioRuntime(runtime));
    }
}

#[derive(Resource)]
pub struct TokioRuntime(tokio::runtime::Runtime);

impl TokioRuntime {
    /// Spawn a future on the Tokio runtime
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.0.spawn(future)
    }

    /// Get a handle to the runtime for spawning tasks
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.0.handle().clone()
    }
}

fn _spawn_async_task(runtime: Res<TokioRuntime>) {
    // Example: spawn an async task
    runtime.spawn(async {
        println!("Async task started");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        println!("Async task completed");
    });
}
