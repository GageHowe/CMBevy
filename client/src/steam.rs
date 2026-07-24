use std::ops::Deref;

use bevy::prelude::*;
use steamworks::{AppId, Client, FriendFlags, SteamAPIInitError};

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
                warn!(
                    "Steam init failed: {}; {}",
                    steam_init_error(&err),
                    steam_init_context()
                );
            }
        }
    }
}

fn steam_init_error(err: &SteamAPIInitError) -> String {
    match err {
        SteamAPIInitError::FailedGeneric(msg)
        | SteamAPIInitError::NoSteamClient(msg)
        | SteamAPIInitError::VersionMismatch(msg) => format!("{err}; {msg}"),
    }
}

fn steam_init_context() -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| format!("unknown ({e})"));
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_ref().and_then(|p| p.parent());
    let dll_ok = exe_dir.is_some_and(|p| p.join("steam_api64.dll").exists());
    let cwd_appid_ok = std::path::Path::new("steam_appid.txt").exists();
    let exe_appid_ok = exe_dir.is_some_and(|p| p.join("steam_appid.txt").exists());
    format!(
        "cwd={cwd}, exe={}, steam_api64.dll={dll_ok}, steam_appid.txt cwd/exe={cwd_appid_ok}/{exe_appid_ok}",
        exe.map_or_else(|| "unknown".into(), |p| p.display().to_string())
    )
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
