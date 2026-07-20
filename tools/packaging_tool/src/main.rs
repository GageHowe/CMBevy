use std::{env, fs, io, path::{Path, PathBuf}, process::Command};

#[cfg(target_os = "windows")]
const PLATFORM: (&str, &[&str]) = (".exe", &["steam_api64.dll", "fmod.dll", "fmodstudio.dll"]);
#[cfg(target_os = "linux")]
const PLATFORM: (&str, &[&str]) = (
    "",
    &["libsteam_api.so", "libfmod.so.14", "libfmodstudio.so.14"],
);
#[cfg(target_os = "macos")]
const PLATFORM: (&str, &[&str]) = (
    "",
    &["libsteam_api.dylib", "libfmod.dylib", "libfmodstudio.dylib"],
);
fn main() -> io::Result<()> {
    let root = env::current_dir()?;
    let dist = root.join("dist");
    if dist.exists() {
        fs::remove_dir_all(&dist)?;
    }
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    let key = bytes.map(|byte| format!("{byte:02x}")).concat();
    cargo(
        &root,
        &format!("run -p pack_assets --release -- --key {key} --dist dist"),
        None,
    )?;
    cargo(&root, "build -p client --release", Some(&key))?;
    cargo(&root, "build -p gameserver --release", None)?;
    let (exe, libs) = PLATFORM;
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("release");
    let copy = |name: &str| fs::copy(target.join(name), dist.join(name));
    for name in ["client", "gameserver"] {
        copy(&format!("{name}{exe}"))?;
    }
    for lib in libs {
        copy(lib)?;
    }
    let archive = format!("criticalmass-{}.zip", env::consts::OS);
    let status = Command::new("zip")
        .current_dir(&root)
        .args(["-rq", &archive, "dist"])
        .status()?;
    if !status.success() {
        return Err(io::Error::other("zip failed"));
    }
    println!("packaged {} and {archive}", dist.display());
    Ok(())
}

fn cargo(root: &Path, args: &str, key: Option<&str>) -> io::Result<()> {
    let mut command = Command::new("cargo");
    command.current_dir(root).args(args.split_whitespace());
    if let Some(key) = key {
        command.env("CM_ASSET_KEY", key);
    }
    if command.status()?.success() {
        Ok(())
    } else {
        Err(io::Error::other("cargo failed"))
    }
}
