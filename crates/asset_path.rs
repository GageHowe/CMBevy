use std::path::{Path, PathBuf};

const HASH_PREFIX: &str = "sha256:";
const FILENAME_HEADER: &str = "x-asset-filename";
const CACHE_DIR: &str = "asset_cache";

// remote assets are just sha256 refs cached to disk the first time we touch them
pub fn resolve_asset_path(path: &str) -> String {
    let (path, fragment) = split_fragment(path);
    let path = if path.starts_with(HASH_PREFIX) {
        cached_asset_rel_path(path)
    } else {
        PathBuf::from(path)
    };
    match fragment {
        Some(fragment) => format!("{}#{fragment}", path.to_string_lossy()),
        None => path.to_string_lossy().into_owned(),
    }
}

pub fn resolve_asset_file_path(path: &str) -> PathBuf {
    let (path, _) = split_fragment(path);
    if !path.starts_with(HASH_PREFIX) {
        return common::config::asset_dir().join(path);
    }
    find_cached_asset(path).unwrap_or_else(|| fetch_asset(path))
}

fn split_fragment(path: &str) -> (&str, Option<&str>) {
    match path.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (path, None),
    }
}

fn fetch_asset(hash: &str) -> PathBuf {
    let dir = cache_dir_path();
    std::fs::create_dir_all(&dir).expect("asset cache dir create failed");
    let response = ureq::get(&format!("{}/assets/{}", common::config::BEACON_URL, hash))
        .call()
        .unwrap_or_else(|err| panic!("failed to fetch asset {hash}: {err}"));
    let path = dir.join(remote_file_name(hash, response.header(FILENAME_HEADER)));
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes)
        .unwrap_or_else(|err| panic!("failed to read asset {hash}: {err}"));
    std::fs::write(&path, bytes)
        .unwrap_or_else(|err| panic!("failed to write cached asset {}: {err}", path.display()));
    path
}

fn find_cached_asset(hash: &str) -> Option<PathBuf> {
    let prefix = hash_key(hash);
    std::fs::read_dir(cache_dir_path())
        .ok()?
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| entry.path())
}

fn remote_file_name(hash: &str, header: Option<&str>) -> String {
    match header.and_then(file_extension) {
        Some(ext) => format!("{}.{}", hash_key(hash), ext),
        None => format!("{}.bin", hash_key(hash)),
    }
}

fn file_extension(name: &str) -> Option<String> {
    let ext = Path::new(name).extension()?.to_string_lossy();
    let ext: String = ext
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect();
    (!ext.is_empty()).then_some(ext)
}

fn hash_key(hash: &str) -> String {
    hash.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect()
}

fn cached_asset_rel_path(hash: &str) -> PathBuf {
    let path = resolve_asset_file_path(hash);
    let asset_dir = common::config::asset_dir();
    path.strip_prefix(&asset_dir)
        .unwrap_or_else(|_| {
            panic!(
                "cached asset {} escaped asset dir {}",
                path.display(),
                asset_dir.display()
            )
        })
        .to_path_buf()
}

fn cache_dir_path() -> PathBuf {
    let asset_dir = common::config::asset_dir();
    let dir = asset_dir.join(CACHE_DIR);
    if dir.exists() || !cfg!(debug_assertions) || !Path::new(CACHE_DIR).exists() {
        return dir;
    }
    let legacy_dir = PathBuf::from(CACHE_DIR);
    std::fs::create_dir_all(&dir).expect("asset cache dir create failed");
    if let Ok(entries) = std::fs::read_dir(&legacy_dir) {
        for entry in entries.flatten() {
            let from = entry.path();
            let to = dir.join(entry.file_name());
            if !to.exists() {
                let _ = std::fs::rename(&from, &to);
            }
        }
    }
    dir
}
