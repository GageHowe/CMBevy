use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use bevy::{
    asset::{
        AssetApp,
        io::{
            AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, Reader, VecReader,
            file::FileAssetReader,
        },
    },
    prelude::*,
    tasks::futures_lite::{Stream, StreamExt},
};

use crate::{DEFAULT_PAK_FILE, Pak, key_from_hex};

pub fn register_asset_pak(app: &mut App) {
    let Some(key) = pak_key() else { return };
    let Some(path) = pak_path() else { return };
    register_asset_pak_with(app, key, path, common::config::asset_dir());
}

pub fn register_asset_pak_with(
    app: &mut App,
    key: [u8; 32],
    path: impl AsRef<Path>,
    fallback_root: impl AsRef<Path>,
) {
    let path = path.as_ref();
    let Ok(pak) = Pak::open(path, key) else {
        warn!("failed to open {}", path.display());
        return;
    };
    let source = PakSource::new(pak, fallback_root.as_ref().to_string_lossy().into_owned());
    app.register_asset_source(
        bevy::asset::io::AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(source.reader())),
    );
}

#[derive(Clone)]
struct PakSource {
    pak: Arc<Pak>,
    dirs: Arc<BTreeMap<PathBuf, Vec<PathBuf>>>,
    fallback_root: String,
}

impl PakSource {
    fn new(pak: Pak, fallback_root: String) -> Self {
        let mut dirs: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
        dirs.entry(PathBuf::new()).or_default();
        for path in pak.entries().keys().map(PathBuf::from) {
            // Bevy directory reads need child listings, so index every parent path once at startup.
            let parent = path.parent().unwrap_or(Path::new("")).to_owned();
            dirs.entry(parent).or_default().insert(path.clone());
            let mut dir = path.parent();
            while let Some(path) = dir {
                let parent = path.parent().unwrap_or(Path::new("")).to_owned();
                dirs.entry(parent).or_default().insert(path.to_owned());
                dir = path.parent();
            }
        }
        Self {
            pak: Arc::new(pak),
            dirs: Arc::new(
                dirs.into_iter()
                    .map(|(path, children)| (path, children.into_iter().collect()))
                    .collect(),
            ),
            fallback_root,
        }
    }

    fn reader(&self) -> PakReader {
        PakReader {
            pak: self.pak.clone(),
            dirs: self.dirs.clone(),
            fallback: FileAssetReader::new(self.fallback_root.clone()),
        }
    }
}

struct PakReader {
    pak: Arc<Pak>,
    dirs: Arc<BTreeMap<PathBuf, Vec<PathBuf>>>,
    fallback: FileAssetReader,
}

impl AssetReader for PakReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let pak_path = path.to_string_lossy().replace('\\', "/");
        match self.pak.read(&pak_path) {
            Ok(bytes) => Ok(Box::new(VecReader::new(bytes)) as Box<dyn Reader>),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => self
                .fallback
                .read(path)
                .await
                .map(|reader| Box::new(reader) as Box<dyn Reader>),
            Err(err) => Err(err.into()),
        }
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.fallback
            .read_meta(path)
            .await
            .map(|reader| Box::new(reader) as Box<dyn Reader>)
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        let mut paths = self.dirs.get(path).cloned().unwrap_or_default();
        if let Ok(mut stream) = self.fallback.read_directory(path).await {
            while let Some(path) = stream.next().await {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        (!paths.is_empty())
            .then(|| Box::new(DirStream(paths)) as Box<PathStream>)
            .ok_or_else(|| AssetReaderError::NotFound(path.to_owned()))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(self.dirs.contains_key(path) || self.fallback.is_directory(path).await.unwrap_or(false))
    }
}

struct DirStream(Vec<PathBuf>);

impl Stream for DirStream {
    type Item = PathBuf;

    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.pop())
    }
}

fn pak_key() -> Option<[u8; 32]> {
    std::env::var("CM_ASSET_KEY")
        .ok()
        .as_deref()
        .or(option_env!("CM_ASSET_KEY"))
        .and_then(|key| key_from_hex(key).ok())
}

fn pak_path() -> Option<PathBuf> {
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.join(DEFAULT_PAK_FILE));
    cwd.filter(|path| path.exists()).or_else(|| {
        let path = common::config::runtime_path(DEFAULT_PAK_FILE);
        path.exists().then_some(path)
    })
}
