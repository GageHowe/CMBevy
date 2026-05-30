use std::ops::Deref;

use bevy::prelude::*;
use steamworks::{AppId, Client, FriendFlags};

const APP_ID: u32 = 3526510;

#[derive(Resource, Clone)]
pub struct SteamClient(pub Client);

impl Deref for SteamClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct SteamworksPlugin;

impl Plugin for SteamworksPlugin {
    fn build(&self, app: &mut App) {
        match Client::init_app(APP_ID) {
            Ok(client) => {
                app.insert_resource(SteamClient(client))
                    .add_systems(PreUpdate, run_callbacks)
                    .add_systems(Startup, print_steam_info);
            }
            Err(err) => {
                warn!("Steam init failed: {err}");
            }
        }
    }
}

fn run_callbacks(client: Option<Res<SteamClient>>) {
    let Some(client) = client else { return };
    client.run_callbacks();
}

fn print_steam_info(client: Option<Res<SteamClient>>) {
    let Some(client) = client else { return };

    let utils = client.utils();
    println!("AppId: {:?}", utils.app_id());
    println!("UI Language: {}", utils.ui_language());

    let appid = AppId(APP_ID);
    let apps = client.apps();
    println!("IsInstalled: {}", apps.is_app_installed(appid));
    println!("InstallDir: {}", apps.app_install_dir(appid));
    println!("BuildId: {}", apps.app_build_id());
    println!("AppOwner: {:?}", apps.app_owner());
    println!("Beta: {:?}", apps.current_beta_name());

    let friends = client.friends();
    for f in friends.get_friends(FriendFlags::IMMEDIATE) {
        println!("Friend: {:?} - {}({:?})", f.id(), f.name(), f.state());
        friends.request_user_information(f.id(), true);
    }
}
