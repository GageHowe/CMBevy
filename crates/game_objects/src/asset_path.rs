use std::path::{Path, PathBuf};

const HASH_PREFIX: &str = "sha256:";
const FILENAME_HEADER: &str = "x-asset-filename";

pub fn resolve_asset_path(path: &str) -> String {
    let Some(remote) = RemoteAsset::parse(path) else {
        return path.to_string();
    };
    let cache_path = cache_path(&remote.hash);
    if !cache_path.exists() {
        fetch_asset(&remote.hash, &cache_path);
    }
    match remote.fragment {
        Some(fragment) => format!("{}#{fragment}", cache_path.to_string_lossy()),
        None => cache_path.to_string_lossy().into_owned(),
    }
}

struct RemoteAsset<'a> {
    hash: &'a str,
    fragment: Option<&'a str>,
}

impl<'a> RemoteAsset<'a> {
    fn parse(path: &'a str) -> Option<Self> {
        let (head, fragment) = match path.split_once('#') {
            Some((head, fragment)) => (head, Some(fragment)),
            None => (path, None),
        };
        head.starts_with(HASH_PREFIX).then_some(Self {
            hash: head,
            fragment,
        })
    }
}

fn fetch_asset(hash: &str, fallback_path: &Path) {
    std::fs::create_dir_all(cache_dir()).expect("asset cache dir create failed");
    let url = format!("{}/assets/{}", common::config::BEACON_URL, hash);
    let response = ureq::get(&url)
        .call()
        .unwrap_or_else(|err| panic!("failed to fetch asset {hash}: {err}"));
    let file_name = response
        .header(FILENAME_HEADER)
        .and_then(sanitize_file_name)
        .unwrap_or_else(|| fallback_file_name(hash));
    let cache_path = cache_dir().join(file_name);
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes)
        .unwrap_or_else(|err| panic!("failed to read asset {hash}: {err}"));
    std::fs::write(&cache_path, bytes).unwrap_or_else(|err| {
        panic!("failed to write cached asset {}: {err}", cache_path.display())
    });
    if cache_path != fallback_path {
        let _ = std::fs::remove_file(fallback_path);
    }
}

fn cache_path(hash: &str) -> PathBuf {
    let dir = cache_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&hash_file_stem(hash)) {
                return entry.path();
            }
        }
    }
    dir.join(fallback_file_name(hash))
}

fn fallback_file_name(hash: &str) -> String {
    format!("{}.bin", hash_file_stem(hash))
}

fn hash_file_stem(hash: &str) -> String {
    hash.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect()
}

fn sanitize_file_name(name: &str) -> Option<String> {
    let file_name = Path::new(name).file_name()?.to_string_lossy();
    let sanitized: String = file_name
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => ch,
            _ => '_',
        })
        .collect();
    (!sanitized.is_empty()).then_some(sanitized)
}

fn cache_dir() -> PathBuf {
    if Path::new("asset_cache").exists() || !cfg!(debug_assertions) {
        PathBuf::from("asset_cache")
    } else {
        PathBuf::from("../asset_cache")
    }
}
