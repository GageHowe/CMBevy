use std::{fs, path::PathBuf, process::Command};

use bevy::prelude::*;
use common::ActiveBindings;

use super::data::Settings;

const SETTINGS_FILE: &str = "settings.toml";

fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CMBevy")
        .join(SETTINGS_FILE)
}

pub fn reveal_settings_file() -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if !path.exists() {
        fs::write(
            &path,
            toml::to_string_pretty(&Settings::default()).unwrap_or_default(),
        )
        .map_err(|err| err.to_string())?;
    }

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer");
        command.arg("/select,").arg(&path);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg("-R").arg(&path);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or_else(|| std::path::Path::new(".")));
        command
    };

    command.spawn().map_err(|err| err.to_string())?;
    Ok(())
}

pub fn load_settings(mut commands: Commands) {
    let path = settings_path();
    let settings = if path.exists() {
        let contents = fs::read_to_string(&path).unwrap_or_default();
        toml::from_str(&contents).unwrap_or_default()
    } else {
        let default = Settings::default();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, toml::to_string_pretty(&default).unwrap_or_default()).ok();
        default
    };

    commands.insert_resource(ActiveBindings::from_settings(
        &settings.keybindings,
        &settings.gamepad_bindings,
    ));
    commands.insert_resource(settings);
}

pub fn save_settings(settings: Res<Settings>) {
    if settings.is_added() {
        return;
    }
    let path = settings_path();
    if let Ok(serialized) = toml::to_string_pretty(&*settings) {
        fs::write(path, serialized).ok();
    }
}

pub fn reset_settings(settings: &mut Settings) {
    fs::remove_file(settings_path()).ok();
    *settings = Settings::default();
}
