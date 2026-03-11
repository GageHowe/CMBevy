use bevy::prelude::*;
use steamworks::Client;
use steamworks::FriendFlags;
// use steamworks::PersonaStateChange;
use steamworks::AppId;
// use crate::slow_update::SlowHz;

/// wraps the Steamworks client. Available as a resource when Steam is running.
#[derive(Resource)]
pub struct SteamClient(pub Client);

pub struct SteamworksPlugin;

impl Plugin for SteamworksPlugin {
    fn build(&self, app: &mut App) {
        match Client::init_app(3526510u32) {
            Ok(client) => {

                let utils = client.utils();
                println!("AppId: {:?}", utils.app_id());

                println!("UI Language: {}", utils.ui_language());

                let apps = client.apps();
                println!("Apps");
                println!("IsInstalled(480): {}", apps.is_app_installed(AppId(480)));
                println!("InstallDir(480): {}", apps.app_install_dir(AppId(480)));
                println!("BuildId: {}", apps.app_build_id());
                println!("AppOwner: {:?}", apps.app_owner());
                println!("Beta: {:?}", apps.current_beta_name());

                let friends = client.friends();
                println!("Friends");
                let list = friends.get_friends(FriendFlags::IMMEDIATE);
                for f in &list {
                    println!("Friend: {:?} - {}({:?})", f.id(), f.name(), f.state());
                    friends.request_user_information(f.id(), true);
                }

                // api good, register self

                app.insert_resource(SteamClient(client));
                app.add_systems(FixedUpdate, pump_callbacks);

            }
            Err(e) => {
                warn!("Steam not available: {e}");
            }
        }
    }
}

fn pump_callbacks(client: Res<SteamClient>) {
    client.0.run_callbacks();
}
