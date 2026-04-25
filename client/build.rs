use std::path::PathBuf;

fn main() {
    let Some(target_dir) = target_dir() else {
        return;
    };

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../dev-assets/lib");

    add_linux_fmod_rpath();
    stage_fmod_runtime(&target_dir);
    copy_steam_runtime(&target_dir);
}

fn target_dir() -> Option<PathBuf> {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").ok()?);
    out_dir.ancestors().nth(3).map(std::path::Path::to_path_buf)
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf()
}

fn target_os() -> String {
    std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default()
}

fn add_linux_fmod_rpath() {
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    if target_os() != "linux" {
        return;
    }
    let arch_dir = match target_arch.as_str() {
        "x86_64" => "x86_64",
        "x86" => "x86",
        "aarch64" => "arm64",
        "arm" => "arm",
        _ => return,
    };
    let sdk_root = repo_root().join("dev-assets/lib/fmodstudioapi20312linux");
    let core_dir = sdk_root.join(format!("api/core/lib/{arch_dir}"));
    let studio_dir = sdk_root.join(format!("api/studio/lib/{arch_dir}"));
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    println!("cargo:rustc-link-search=native={}", core_dir.display());
    println!("cargo:rustc-link-search=native={}", studio_dir.display());
}

fn stage_fmod_runtime(target_dir: &std::path::Path) {
    match target_os().as_str() {
        "linux" => {
            let sdk_root = repo_root().join("dev-assets/lib/fmodstudioapi20312linux");
            let core_lib = sdk_root.join("api/core/lib/x86_64/libfmod.so.14");
            let studio_lib = sdk_root.join("api/studio/lib/x86_64/libfmodstudio.so.14");
            stage_file(&core_lib, &target_dir.join("libfmod.so.14"));
            stage_file(&studio_lib, &target_dir.join("libfmodstudio.so.14"));
        }
        "windows" => {
            let lib_root = repo_root().join("dev-assets/lib");
            stage_file(&lib_root.join("fmod.dll"), &target_dir.join("fmod.dll"));
            stage_file(&lib_root.join("fmodstudio.dll"), &target_dir.join("fmodstudio.dll"));
        }
        _ => {}
    }
}

fn stage_file(src: &std::path::Path, dst: &std::path::Path) {
    if !src.exists() {
        return;
    }
    if std::fs::symlink_metadata(dst).is_ok() {
        let _ = std::fs::remove_file(dst);
    }
    let _ = std::fs::copy(src, dst);
}

fn copy_steam_runtime(target_dir: &std::path::Path) {
    let build_dir = target_dir.join("build");
    let lib_name = match target_os().as_str() {
        "windows" => "steam_api64.dll",
        "linux" => "libsteam_api.so",
        _ => return,
    };
    if let Ok(entries) = std::fs::read_dir(build_dir) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("out").join(lib_name);
            if candidate.exists() {
                let _ = std::fs::copy(&candidate, target_dir.join(lib_name));
                return;
            }
        }
    }
}
