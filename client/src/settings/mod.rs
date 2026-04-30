mod controls;
mod data;
mod persistence;
mod runtime;
mod ui;

use bevy::prelude::*;
use common::{ActiveKeyBindings, KeyBindings};
pub use controls::ControlsCapture;
pub use data::{
    DisplayMode, PhysicsInterp, PhysicsSubsteps, Settings, SettingsSection, ShadowQuality, SsaoQuality,
    VsyncMode,
};
pub use ui::show_settings_ui;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Settings>()
            .register_type::<KeyBindings>()
            .insert_resource(ControlsCapture::default())
            .init_resource::<ActiveKeyBindings>()
            .add_systems(Startup, persistence::load_settings)
            .add_systems(PostUpdate, runtime::apply_settings.run_if(resource_changed::<Settings>))
            .add_systems(
                PostUpdate,
                runtime::sync_active_keybindings.run_if(resource_changed::<Settings>),
            )
            .add_systems(PostUpdate, runtime::sync_dynamic_graphics_settings)
            .add_systems(
                PostUpdate,
                persistence::save_settings.run_if(resource_changed::<Settings>),
            )
            .add_systems(Last, runtime::apply_fps_cap);

        runtime::build_render(app);
    }
}
