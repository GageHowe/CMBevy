use bevy::prelude::*;
use common::KeyBindings;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Reflect, PartialEq, Default)]
pub enum PhysicsSubsteps {
    #[default]
    One,
    Two,
    Four,
}

impl PhysicsSubsteps {
    pub fn count(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
pub enum PhysicsInterp {
    Off,
    Interpolate,
    Extrapolate,
    #[default]
    RotationOnly,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
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
pub enum DisplayMode {
    Windowed,
    #[default]
    BorderlessFullscreen,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
pub enum SsaoQuality {
    Off,
    #[default]
    Medium,
    High,
    Ultra,
}

#[derive(Serialize, Deserialize, Clone, Reflect, PartialEq, Default)]
pub enum ShadowQuality {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    Graphics,
    Audio,
    Input,
    Controls,
}

#[derive(Resource, Serialize, Deserialize, Clone, Reflect)]
#[serde(default)]
#[reflect(Resource)]
pub struct Settings {
    pub keybindings: KeyBindings,
    pub mouse_sensitivity: f32,
    pub zoom_sensitivity_blend: f32,
    pub vehicle_pitch_yaw_sensitivity: f32,
    pub preserve_look_across_planet_snap: bool,
    pub audio_output_device: String,
    pub fmod_buffer_size: u32,
    pub ui_scale: f32,
    pub anti_aliasing: bool,
    pub auto_exposure: bool,
    pub bloom: bool,
    pub motion_blur: bool,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub ssao_quality: SsaoQuality,
    pub shadow_quality: ShadowQuality,
    pub fps_cap: u16,
    pub gamma: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub fov: f32,
    pub physics_interp: PhysicsInterp,
    pub physics_substeps: PhysicsSubsteps,
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
            mouse_sensitivity: 0.002,
            zoom_sensitivity_blend: 1.0,
            vehicle_pitch_yaw_sensitivity: 0.002,
            preserve_look_across_planet_snap: false,
            audio_output_device: String::new(),
            fmod_buffer_size: 256,
            ui_scale: 1.25,
            anti_aliasing: true,
            auto_exposure: true,
            bloom: true,
            motion_blur: false,
            bloom_intensity: 0.5,
            bloom_threshold: 0.5,
            ssao_quality: SsaoQuality::Medium,
            shadow_quality: ShadowQuality::Medium,
            fps_cap: 0,
            gamma: 1.2,
            contrast: 1.1,
            saturation: 1.2,
            fov: 90.0,
            physics_interp: PhysicsInterp::RotationOnly,
            physics_substeps: PhysicsSubsteps::One,
            cinematic_mode: false,
            debug_panel: false,
            debug_render: false,
            vsync: VsyncMode::AutoNoVsync,
            display_mode: DisplayMode::BorderlessFullscreen,
        }
    }
}
