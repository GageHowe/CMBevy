use std::path::PathBuf;

fn main() {
    let Some(target_dir) = target_dir() else {
        return;
    };

    add_linux_fmod_rpath();
    stage_assets(&target_dir);
    stage_linux_fmod_libs(&target_dir);
    copy_steam_runtime(&target_dir);

    println!("cargo:rerun-if-changed=build.rs");
}

fn target_dir() -> Option<PathBuf> {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").ok()?);
    out_dir.ancestors().nth(3).map(std::path::Path::to_path_buf)
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf()
}

fn add_linux_fmod_rpath() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    if target_os != "linux" {
        return;
    }
    let arch_dir = match target_arch.as_str() {
        "x86_64" => "x86_64",
        "x86" => "x86",
        "aarch64" => "arm64",
        "arm" => "arm",
        _ => return,
    };
    let sdk_root = repo_root().join("assets/lib/fmodstudioapi20312linux");
    let core_dir = sdk_root.join(format!("api/core/lib/{arch_dir}"));
    let studio_dir = sdk_root.join(format!("api/studio/lib/{arch_dir}"));
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    println!("cargo:rustc-link-search=native={}", core_dir.display());
    println!("cargo:rustc-link-search=native={}", studio_dir.display());
}

// libfmodstudio.so has RUNPATH=$ORIGIN, meaning it looks for libfmod.so in the same directory
// it was loaded from. Staging both FMOD libs next to the executable keeps runtime loading simple.
fn stage_linux_fmod_libs(target_dir: &std::path::Path) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::symlink;

        let sdk_root = repo_root().join("assets/lib/fmodstudioapi20312linux");
        let core_lib = sdk_root.join("api/core/lib/x86_64");
        let studio_lib = sdk_root.join("api/studio/lib/x86_64");

        for (src, name) in [
            (core_lib.join("libfmod.so.14"), "libfmod.so.14"),
            (
                studio_lib.join("libfmodstudio.so.14"),
                "libfmodstudio.so.14",
            ),
        ] {
            let dst = target_dir.join(name);
            if !dst.exists() {
                let _ = symlink(&src, &dst);
            }
        }
    }
}

// Keep runtime assets next to the built executable so both cargo-run and shipped builds use
// the same path convention.
fn stage_assets(target_dir: &std::path::Path) {
    let link = target_dir.join("assets");
    if !link.exists() {
        #[cfg(unix)]
        let _ = std::os::unix::fs::symlink(repo_root().join("assets"), &link);
        #[cfg(windows)]
        let _ = std::os::windows::fs::symlink_dir(repo_root().join("assets"), &link);
    }
}

fn copy_steam_runtime(target_dir: &std::path::Path) {
    let build_dir = target_dir.join("build");
    let lib_name = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => "steam_api64.dll",
        Ok("linux") => "libsteam_api.so",
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
