use bevy::prelude::*;
use std::collections::HashMap;

const ASSETS: &[&str] = &[
    "models/companion_cube.glb",
    "models/companion_cube_2.glb",
];

#[derive(Resource, Default)]
pub struct CMAssets(HashMap<&'static str, UntypedHandle>);

impl CMAssets {
    pub fn get<A: Asset>(&self, path: &'static str) -> Handle<A> {
        self.0[path].clone().typed()
    }
}

pub struct CMAssetPlugin;
impl Plugin for CMAssetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_assets);
    }
}

fn load_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let map = ASSETS
        .iter()
        .map(|&path| (path, asset_server.load_untyped(path).untyped()))
        .collect();
    commands.insert_resource(CMAssets(map));
}
