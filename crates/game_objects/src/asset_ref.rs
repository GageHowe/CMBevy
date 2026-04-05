use common::AssetRef;
use std::path::{Path, PathBuf};

pub fn resolve_asset_path(asset: &AssetRef) -> String {
    match asset {
        AssetRef::Path(path) => path.clone(),
        AssetRef::Hash { hash, .. } => {
            let cache_path = cache_path(asset).expect("hashed asset missing cache file name");
            if !cache_path.exists() {
                fetch_asset(hash, &cache_path);
            }
            with_fragment(cache_path, asset)
        }
    }
}

fn cache_path(asset: &AssetRef) -> Option<PathBuf> {
    let file_name = asset.cache_file_name()?;
    Some(cache_dir().join(file_name))
}

fn with_fragment(path: PathBuf, asset: &AssetRef) -> String {
    let raw = path.to_string_lossy().into_owned();
    match asset.split_path().1 {
        Some(fragment) => format!("{raw}#{fragment}"),
        None => raw,
    }
}

fn fetch_asset(hash: &str, path: &Path) {
    std::fs::create_dir_all(cache_dir()).expect("asset cache dir create failed");
    let url = format!(
        "{}/assets/{}",
        common::config::BEACON_URL,
        hash.replace(':', "%3A")
    );
    let response = ureq::get(&url)
        .call()
        .unwrap_or_else(|err| panic!("failed to fetch asset {hash}: {err}"));
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes)
        .unwrap_or_else(|err| panic!("failed to read asset {hash}: {err}"));
    std::fs::write(path, bytes).unwrap_or_else(|err| {
        panic!("failed to write cached asset {}: {err}", path.display())
    });
}

fn cache_dir() -> PathBuf {
    if Path::new("asset_cache").exists() || !cfg!(debug_assertions) {
        PathBuf::from("asset_cache")
    } else {
        PathBuf::from("../asset_cache")
    }
}
