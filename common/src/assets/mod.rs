use bevy::asset::embedded_asset;
use bevy::prelude::*;

pub struct CMAssetPlugin;
impl Plugin for CMAssetPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "models/companion_cube.glb");
        embedded_asset!(app, "models/spaceship.obj");
    }
}
