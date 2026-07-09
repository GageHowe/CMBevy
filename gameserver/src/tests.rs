use std::{io, path::PathBuf};

use bevy::prelude::*;
use master_plugin::register_asset_pak_with;

#[test]
fn loads_secret_asset_from_pak() -> io::Result<()> {
    let root =
        std::env::temp_dir().join(format!("cmbevy_gameserver_pak_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("encrypted"))?;
    let pak_path = root.join(asset_pak::DEFAULT_PAK_FILE);
    let asset_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| io::Error::other("bad manifest dir"))?
        .join("assets/textures/crosshairs/crosshair001.png");
    let key = [9; 32];
    asset_pak::write(
        &pak_path,
        [(
            "textures/crosshairs/crosshair001.png".into(),
            asset_path.clone(),
            false,
        )],
        key,
    )?;

    let mut app = App::new();
    register_asset_pak_with(
        &mut app,
        key,
        &pak_path,
        gameplay::level::default_asset_dir(),
    );
    let pak = asset_pak::Pak::open(&pak_path, key)?;
    assert_eq!(
        pak.read("textures/crosshairs/crosshair001.png")?,
        std::fs::read(asset_path)?
    );
    let _ = std::fs::remove_dir_all(root);
    Ok(())
}
