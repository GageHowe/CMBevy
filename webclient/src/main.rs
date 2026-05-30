use bevy::prelude::*;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.0, 0.0, 1.0)))
        // .add_plugins(MinimalPlugins)
        .add_plugins(DefaultPlugins.build().disable::<bevy::audio::AudioPlugin>())
        // .add_plugins(window::WindowPlugin { ..default() })
        // .add_plugins(bevy::scene::ScenePlugin)
        // .add_plugins(PipelinedRenderingPlugin)
        // .add_plugins((
        //     WinitPlugin,
        //     WindowPlugin,
        //     RenderPlugin,
        //     Core3dPlugin, // Required for Camera3d and 3D rendering
        // ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, _asset_server: Res<AssetServer>) {
    commands.spawn(Camera3d::default());

    // commands.spawn(Sprite::from_image(asset_server.load("branding/icon.png")));
}
