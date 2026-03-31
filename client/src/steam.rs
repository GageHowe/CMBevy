use bevy::prelude::*;
use bevy_steamworks::{AppId, Client, FriendFlags};

pub struct SteamworksPlugin;

impl Plugin for SteamworksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, print_steam_info);
    }
}

fn print_steam_info(client: Option<Res<Client>>) {
    let Some(client) = client else { return };

    let utils = client.utils();
    println!("AppId: {:?}", utils.app_id());
    println!("UI Language: {}", utils.ui_language());

    let appid = AppId(3526510);
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
