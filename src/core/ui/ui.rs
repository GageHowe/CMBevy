use bevy::prelude::*;
// use bevy::ui::
// use bevy::window::*;

pub struct UIPlugin;

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crosshair);
    }
}

pub fn spawn_crosshair(mut commands: Commands) {
    // let window = window_query.single().unwrap();
    let crosshair_size = 2.0;

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|parent| {
            parent.spawn((
                Node {
                    width: Val::Px(crosshair_size),
                    height: Val::Px(crosshair_size),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
            ));
        });
}
