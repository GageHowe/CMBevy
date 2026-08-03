pub(crate) mod controls;
mod data;
pub(crate) mod persistence;
mod runtime;

use bevy::prelude::*;
use common::{
    ActiveBindings, GamepadBindings, KeyBindings, PromptDeviceMode, PromptDevicePreference,
};
pub use controls::{CaptureDevice, ControlsCapture};
pub use data::{
    DisplayMode, PhysicsInterp, Settings, SettingsSection, ShadowQuality, SsaoQuality, VsyncMode,
};

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Settings>()
            .register_type::<PromptDeviceMode>()
            .register_type::<PromptDevicePreference>()
            .register_type::<KeyBindings>()
            .register_type::<GamepadBindings>()
            .insert_resource(ControlsCapture::default())
            .init_resource::<ActiveBindings>()
            .init_resource::<PromptDevicePreference>()
            .add_systems(Startup, persistence::load_settings)
            .add_systems(
                PostUpdate,
                runtime::apply_settings.run_if(resource_changed::<Settings>),
            )
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
    }
}
