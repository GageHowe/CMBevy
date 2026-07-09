#![allow(linker_messages)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    runtime: Vec<String>,
    pak: Vec<PakRule>,
}

#[derive(Deserialize)]
struct PakRule {
    path: String,
    compressed: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut dist = PathBuf::from("dist");
    let mut assets = PathBuf::from("assets");
    let mut config_path = PathBuf::from("assets/package.toml");
    let mut key = std::env::var("CM_ASSET_KEY").ok();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = || format!("missing value after {arg}");
        match arg.as_str() {
            "--dist" => dist = args.next().ok_or_else(value)?.into(),
            "--assets" => assets = args.next().ok_or_else(value)?.into(),
            "--config" => config_path = args.next().ok_or_else(value)?.into(),
            "--key" => key = Some(args.next().ok_or_else(value)?),
            _ => return Err(format!("unknown arg {arg}")),
        }
    }

    let key = asset_pak::key_from_hex(
        key.as_deref()
            .ok_or("set CM_ASSET_KEY or pass --key with 64 hex chars")?,
    )?;
    let config: Config = toml::from_str(
        &fs::read_to_string(&config_path)
            .map_err(|err| format!("{}: {err}", config_path.display()))?,
    )
    .map_err(|err| err.to_string())?;

    let dist_assets = dist.join("assets");
    fs::create_dir_all(&dist_assets).map_err(|err| err.to_string())?;
    for dir in &config.runtime {
        copy_dir(&assets.join(dir), &dist_assets.join(dir))?;
    }

    let mut files = BTreeMap::new();
    collect_pak(&dist_assets, &dist_assets, &config.pak, &mut files)?;
    let count = files.len();
    let remove: Vec<_> = files.values().map(|(path, _)| path.clone()).collect();
    asset_pak::write(
        dist.join(asset_pak::DEFAULT_PAK_FILE),
        files
            .into_iter()
            .map(|(path, (src, compressed))| (path, src, compressed)),
        key,
    )
    .map_err(|err| err.to_string())?;
    for path in remove {
        fs::remove_file(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    }
    prune_empty_dirs(&dist_assets, false)?;
    println!("packed {count} assets");
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|err| err.to_string())?;
    for entry in fs::read_dir(src).map_err(|err| format!("{}: {err}", src.display()))? {
        let path = entry.map_err(|err| err.to_string())?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.starts_with('.') || name == "README.md" {
            continue;
        }
        if path.is_dir() {
            copy_dir(&path, &dst.join(name))?;
        } else if path.is_file() {
            fs::copy(&path, dst.join(name)).map_err(|err| format!("{}: {err}", path.display()))?;
        }
    }
    Ok(())
}

fn collect_pak(
    root: &Path,
    dir: &Path,
    rules: &[PakRule],
    files: &mut BTreeMap<String, (PathBuf, bool)>,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.is_dir() {
            collect_pak(root, &path, rules, files)?;
        } else if path.is_file() {
            let logical = path
                .strip_prefix(root)
                .map_err(|err| err.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if let Some(rule) = rules.iter().find(|rule| matches(&rule.path, &logical)) {
                files.insert(logical, (path.clone(), rule.compressed));
            }
        }
    }
    Ok(())
}

fn matches(pattern: &str, path: &str) -> bool {
    pattern
        .strip_suffix("/**")
        .is_some_and(|prefix| path == prefix || path.starts_with(&format!("{prefix}/")))
        || pattern == path
}

fn prune_empty_dirs(dir: &Path, remove_self: bool) -> Result<bool, String> {
    let mut empty = true;
    for entry in fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))? {
        let path = entry.map_err(|err| err.to_string())?.path();
        empty &= path.is_dir() && prune_empty_dirs(&path, true)?;
    }
    if empty && remove_self {
        fs::remove_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    Ok(empty)
}
