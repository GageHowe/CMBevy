use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // OUT_DIR = target/{profile}/build/client-{hash}/out — up 3 levels = target/{profile}/
    let Some(target_dir) = out_dir.ancestors().nth(3) else { return };

    add_linux_fmod_rpath();
    copy_steam_dll(&out_dir, target_dir);

    println!("cargo:rerun-if-changed=build.rs");
}

fn add_linux_fmod_rpath() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo_root = manifest_dir.parent().unwrap_or(&manifest_dir);
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
    let sdk_root = repo_root.join("assets/lib/fmodstudioapi20312linux");
    let core_dir = sdk_root.join(format!("api/core/lib/{arch_dir}"));
    let studio_dir = sdk_root.join(format!("api/studio/lib/{arch_dir}"));
    println!("cargo:rustc-link-search=native={}", core_dir.display());
    println!("cargo:rustc-link-search=native={}", studio_dir.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", core_dir.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", studio_dir.display());
}

fn copy_steam_dll(out_dir: &std::path::Path, target_dir: &std::path::Path) {
    // steamworks-sys puts the redistributable next to its own OUT_DIR sibling — find it.
    let build_dir = out_dir.ancestors().nth(2).unwrap_or(out_dir); // target/{profile}/build/
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
