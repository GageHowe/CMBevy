use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // OUT_DIR = target/{profile}/build/client-{hash}/out — up 3 levels = target/{profile}/
    let Some(target_dir) = out_dir.ancestors().nth(3) else { return };

    copy_fmod_dlls(target_dir);
    copy_steam_dll(&out_dir, target_dir);

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FMOD_DIRECTORY");
}

fn copy_fmod_dlls(target_dir: &std::path::Path) {
    let Some(fmod_dir) = find_fmod_dir() else { return };
    for dll in ["fmod.dll", "fmodstudio.dll"] {
        let src = fmod_dir.join(dll);
        if src.exists() {
            let _ = std::fs::copy(&src, target_dir.join(dll));
        }
    }
}

fn find_fmod_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FMOD_DIRECTORY").map(PathBuf::from) {
        if dir.exists() {
            return Some(dir);
        }
    }

    for path in [
        PathBuf::from(r"C:\Program Files (x86)\FMOD SoundSystem\FMOD Studio API Windows\api\core\lib\x64"),
        PathBuf::from(r"D:\Program Files (x86)\FMOD SoundSystem\FMOD Studio API Windows\api\core\lib\x64"),
    ] {
        if path.exists() {
            return Some(path);
        }
    }

    None
}

fn copy_steam_dll(out_dir: &std::path::Path, target_dir: &std::path::Path) {
    // steamworks-sys puts steam_api64.dll in its own OUT_DIR sibling — find it.
    let build_dir = out_dir.ancestors().nth(2).unwrap_or(out_dir); // target/{profile}/build/
    let dll_name = "steam_api64.dll";
    if let Ok(entries) = std::fs::read_dir(build_dir) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("out").join(dll_name);
            if candidate.exists() {
                let _ = std::fs::copy(&candidate, target_dir.join(dll_name));
                return;
            }
        }
    }
}

