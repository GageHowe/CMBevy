use bevy::prelude::Reflect;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Reflect, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum AssetRef {
    Path(String),
    Hash { hash: String, path: String },
}

impl Default for AssetRef {
    fn default() -> Self {
        Self::Path(String::new())
    }
}

impl AssetRef {
    pub fn source_path(&self) -> &str {
        match self {
            Self::Path(path) => path,
            Self::Hash { path, .. } => path,
        }
    }

    pub fn hash(&self) -> Option<&str> {
        match self {
            Self::Path(_) => None,
            Self::Hash { hash, .. } => Some(hash),
        }
    }

    pub fn split_path(&self) -> (&str, Option<&str>) {
        let path = self.source_path();
        match path.split_once('#') {
            Some((path, fragment)) => (path, Some(fragment)),
            None => (path, None),
        }
    }

    pub fn cache_file_name(&self) -> Option<String> {
        let hash = self.hash()?;
        let (path, _) = self.split_path();
        let file_name = Path::new(path).file_name()?.to_string_lossy();
        Some(format!("{}-{}", sanitize_hash(hash), file_name))
    }
}

fn sanitize_hash(hash: &str) -> String {
    hash.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect()
}
