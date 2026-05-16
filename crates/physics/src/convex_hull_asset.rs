use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use rapier3d::prelude::{Collider, ColliderBuilder, Pose, SharedShape};
use std::{fs, path::Path};
// use rapier3d::

/// custom asset type for convex hulls
#[derive(Asset, TypePath)]
pub struct ConvexHullAsset(pub Collider);

/// custom AssetLoader implementation for .obj convex hulls
#[derive(Default, TypePath)]
pub struct ConvexHullAssetLoader;
impl AssetLoader for ConvexHullAssetLoader {
    type Asset = ConvexHullAsset;
    /// Uniform scale applied to all vertices at load time.
    type Settings = f32;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &f32,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<ConvexHullAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let text = std::str::from_utf8(&bytes)?;
        parse_obj_compound(text, *settings)
            .map(ConvexHullAsset)
            .ok_or_else(|| "failed to build convex hulls from OBJ".into())
    }

    /// derived function that tells the asset plugin to associate .obj files with this loader
    fn extensions(&self) -> &[&str] {
        &["obj"]
    }
}

/// converts the string contents of a .obj file into a rapier3d Collider
pub fn parse_obj_compound(text: &str, scale: f32) -> Option<Collider> {
    let s = if scale == 0.0 { 1.0 } else { scale };
    let mut shapes: Vec<(Pose, SharedShape)> = Vec::new();
    let mut verts: Vec<Vec3> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("o ") {
            if !verts.is_empty() {
                shapes.push((Pose::IDENTITY, SharedShape::convex_hull(&verts)?));
                verts.clear();
            }
        } else if line.starts_with("v ") {
            let mut p = line[2..].split_whitespace();
            let x: f32 = p.next()?.parse().ok()?;
            let y: f32 = p.next()?.parse().ok()?;
            let z: f32 = p.next()?.parse().ok()?;
            verts.push(Vec3::new(x * s, y * s, z * s));
        }
    }
    if !verts.is_empty() {
        shapes.push((Pose::IDENTITY, SharedShape::convex_hull(&verts)?));
    }

    if shapes.is_empty() {
        return None;
    }
    Some(ColliderBuilder::compound(shapes).build())
}

pub fn load_convex_hull_blocking(path: impl AsRef<Path>, scale: f32) -> Option<Collider> {
    let text = fs::read_to_string(path).ok()?;
    parse_obj_compound(&text, scale)
}

/// plugin that registers the custom Asset and AssetLoader
pub struct ConvexHullPlugin;
impl Plugin for ConvexHullPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<ConvexHullAsset>()
            .init_asset_loader::<ConvexHullAssetLoader>();
    }
}
