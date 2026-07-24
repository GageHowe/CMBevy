use bevy::prelude::*;
use common::{GamepadBindings, KeyBindings, PromptDeviceMode};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
/// Visual smoothing mode applied between fixed physics ticks.
pub enum PhysicsInterp {
    Off,
    Interpolate,
    Extrapolate,
    #[default]
    Balanced,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
/// Frontend-facing present mode choice mapped to Bevy window settings.
pub enum VsyncMode {
    AutoVsync,
    #[default]
    AutoNoVsync,
    Fifo,
    FifoRelaxed,
    Immediate,
    Mailbox,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
/// Window/display mode exposed in the settings UI.
pub enum DisplayMode {
    Windowed,
    #[default]
    BorderlessFullscreen,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
/// SSAO quality preset exposed by the graphics menu.
pub enum SsaoQuality {
    Off,
    #[default]
    Medium,
    High,
    Ultra,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
/// Shadow quality preset exposed by the graphics menu.
pub enum ShadowQuality {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
/// Top-level settings tab currently selected in the pause/settings UI.
pub enum SettingsSection {
    #[default]
    Graphics,
    Audio,
    Input,
    Controls,
    Misc,
}

#[derive(Resource, Serialize, Deserialize, Clone, Reflect)]
#[serde(default)]
#[reflect(Resource)]
/// Persistent user settings shared across graphics, audio, controls, and debug options.
pub struct Settings {
    pub keybindings: KeyBindings,
    pub gamepad_bindings: GamepadBindings,
    pub mouse_sensitivity: f32,
    pub zoom_sensitivity_blend: f32,
    pub vehicle_pitch_yaw_sensitivity: f32,
    pub gamepad_look_sensitivity: f32,
    pub gamepad_move_deadzone: f32,
    pub gamepad_look_deadzone: f32,
    pub gamepad_invert_y: bool,
    pub prompt_device_mode: PromptDeviceMode,
    pub audio_output_device: String,
    pub fmod_buffer_size: u32,
    pub ui_scale: f32,
    pub reticle_scale: f32,
    pub anti_aliasing: bool,
    pub auto_exposure: bool,
    pub bloom: bool,
    pub motion_blur: bool,
    pub motion_blur_shutter_angle: f32,
    pub vignette: bool,
    pub vignette_intensity: f32,
    pub lens_distortion: bool,
    pub lens_distortion_intensity: f32,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub ssao_quality: SsaoQuality,
    pub shadow_quality: ShadowQuality,
    pub fps_cap: u16,
    pub gamma: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub outline_red: f32,
    pub outline_green: f32,
    pub outline_blue: f32,
    pub outline_opacity: f32,
    pub fov: f32,
    pub physics_interp: PhysicsInterp,
    pub cinematic_mode: bool,
    pub debug_panel: bool,
    pub debug_render: bool,
    pub vsync: VsyncMode,
    pub display_mode: DisplayMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            keybindings: KeyBindings::default(),
            gamepad_bindings: GamepadBindings::default(),
            mouse_sensitivity: 0.002,
            zoom_sensitivity_blend: 1.0,
            vehicle_pitch_yaw_sensitivity: 0.002,
            gamepad_look_sensitivity: 3.0,
            gamepad_move_deadzone: 0.2,
            gamepad_look_deadzone: 0.15,
            gamepad_invert_y: false,
            prompt_device_mode: PromptDeviceMode::Both,
            audio_output_device: String::new(),
            fmod_buffer_size: 256,
            ui_scale: 1.25,
            reticle_scale: 1.0,
            anti_aliasing: true,
            auto_exposure: true,
            bloom: true,
            motion_blur: false,
            motion_blur_shutter_angle: 0.5,
            vignette: true,
            vignette_intensity: 0.18,
            lens_distortion: true,
            lens_distortion_intensity: 0.03,
            bloom_intensity: 0.5,
            bloom_threshold: 0.5,
            ssao_quality: SsaoQuality::Medium,
            shadow_quality: ShadowQuality::Medium,
            fps_cap: 0,
            gamma: 1.0,
            contrast: 1.0,
            saturation: 1.2,
            outline_red: 0.5,
            outline_green: 0.5,
            outline_blue: 0.5,
            outline_opacity: 0.03,
            fov: 90.0,
            physics_interp: PhysicsInterp::Interpolate,
            cinematic_mode: false,
            debug_panel: false,
            debug_render: false,
            vsync: VsyncMode::AutoNoVsync,
            display_mode: DisplayMode::BorderlessFullscreen,
        }
    }
}
