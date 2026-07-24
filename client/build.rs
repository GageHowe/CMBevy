use std::path::{Path, PathBuf};

fn main() {
    let Some(target_dir) = target_dir() else {
        return;
    };
    let root = repo_root();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../assets/lib");
    println!("cargo:rerun-if-env-changed=CM_ASSET_KEY");

    if std::env::var_os("FMOD_LIB_DIR").is_none() && std::env::var_os("FMOD_SDK_DIR").is_none() {
        add_fmod_link_paths(&root, &os);
    }
    for (src, dst) in runtime_libs(&root, &os) {
        stage_file(&src, &target_dir.join(dst));
    }
    stage_steam_appid(&target_dir);
    copy_steam_runtime(&target_dir, &os);
}

fn target_dir() -> Option<PathBuf> {
    PathBuf::from(std::env::var("OUT_DIR").ok()?)
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
}

fn repo_root() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf()
}

fn add_fmod_link_paths(root: &Path, os: &str) {
    match os {
        "macos" => {
            println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path");
            add_fmod_dir(
                root.join("assets/lib/mac"),
                &["libfmod.dylib", "libfmodstudio.dylib"],
            );
        }
        "windows" => add_fmod_dir(
            root.join("assets/lib/windows"),
            &["fmod_vc.lib", "fmodstudio_vc.lib"],
        ),
        "linux" => {
            let dir = root.join("assets/lib/linux");
            for file in ["libfmod.so", "libfmodstudio.so"] {
                if !dir.join(file).exists() {
                    panic!("{} not found under {}", file, dir.display());
                }
            }
            println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
            println!("cargo:rustc-link-search=native={}", dir.display());
        }
        _ => {}
    }
}

fn add_fmod_dir(dir: PathBuf, required_files: &[&str]) {
    for file in required_files {
        if !dir.join(file).exists() {
            panic!(
                "{} not found. Put FMOD programmer API libraries under {} or set FMOD_LIB_DIR.",
                file,
                dir.display()
            );
        }
    }
    println!("cargo:rustc-link-search=native={}", dir.display());
}

fn runtime_libs(root: &Path, os: &str) -> Vec<(PathBuf, &'static str)> {
    let lib = root.join("assets/lib");
    match os {
        "macos" => vec![
            (lib.join("mac/libfmod.dylib"), "libfmod.dylib"),
            (lib.join("mac/libfmodstudio.dylib"), "libfmodstudio.dylib"),
        ],
        "windows" => vec![
            (lib.join("windows/fmod.dll"), "fmod.dll"),
            (lib.join("windows/fmodstudio.dll"), "fmodstudio.dll"),
        ],
        "linux" => vec![
            (lib.join("linux/libfmod.so.14"), "libfmod.so.14"),
            (lib.join("linux/libfmodstudio.so.14"), "libfmodstudio.so.14"),
        ],
        _ => Vec::new(),
    }
}

fn stage_file(src: &Path, dst: &Path) {
    if src.exists() {
        let _ = std::fs::remove_file(dst);
        let _ = std::fs::copy(src, dst);
    }
}

fn stage_steam_appid(target_dir: &Path) {
    let _ = std::fs::write(target_dir.join("steam_appid.txt"), "3526510\n");
}

fn copy_steam_runtime(target_dir: &Path, os: &str) {
    let lib_name = match os {
        "macos" => "libsteam_api.dylib",
        "windows" => "steam_api64.dll",
        "linux" => "libsteam_api.so",
        _ => return,
    };
    if let Ok(entries) = std::fs::read_dir(target_dir.join("build")) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("out").join(lib_name);
            if candidate.exists() {
                let _ = std::fs::copy(&candidate, target_dir.join(lib_name));
                return;
            }
        }
    }
}
