use bevy::{
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, /* WindowMode*/ WindowResolution},
};

pub struct WindowSettingsPlugin;

impl Plugin for WindowSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, init_window);
        app.add_systems(Update, toggle_cursor_lock);
    }
}

fn init_window(mut window_query: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = window_query.single_mut() {
        window.resolution = WindowResolution::new(1920, 1080);
        // window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    }
}

fn toggle_cursor_lock(
    mut cursor_options: Single<&mut CursorOptions>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    // grab and hide cursor on left click
    if mouse.just_pressed(MouseButton::Left) {
        cursor_options.visible = false;
        cursor_options.grab_mode = CursorGrabMode::Locked;
    }

    // release and show cursor on Escape
    if keys.just_pressed(KeyCode::Escape) {
        cursor_options.visible = true;
        cursor_options.grab_mode = CursorGrabMode::None;
    }
}
