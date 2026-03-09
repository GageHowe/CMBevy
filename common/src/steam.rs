use bevy::prelude::*;
use steamworks::Client;

/// Wraps the Steamworks client. Available as a resource when Steam is running.
#[derive(Resource)]
pub struct SteamClient(pub Client);

pub struct SteamworksPlugin;

impl Plugin for SteamworksPlugin {
    fn build(&self, app: &mut App) {
        match Client::init_app(3526510u32) {
            Ok(client) => {
                app.insert_resource(SteamClient(client))
                    .add_systems(Update, pump_callbacks);
            }
            Err(e) => {
                warn!("Steam not available: {e}. Cloud saves disabled.");
            }
        }
    }
}

fn pump_callbacks(client: Res<SteamClient>) {
    client.0.run_callbacks();
}
