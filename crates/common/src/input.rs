use bevy::prelude::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct ItemInput {
    pub weapon: Option<u64>,
    pub primary: bool,
    pub primary_pressed: bool,
    pub secondary: bool,
    pub secondary_pressed: bool,
    pub reload_pressed: bool,
    pub tick: u64,
    pub origin: Vec3,
    pub aim_dir: Vec3,
}

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct PawnInput {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub jump: bool,
    pub slide: bool,
    pub ability1: bool,
    pub ability2: bool,
    pub ability1_pressed: bool,
    pub melee_pressed: bool,
    pub item: ItemInput,
    /// pawn-local yaw from the YawPivot at input time (radians, absolute)
    pub look_yaw: f32,
    /// camera pitch from the PitchPivot at input time (radians, absolute)
    pub look_pitch: f32,
    /// mouse-driven yaw delta this tick (radians)
    pub yaw: f32,
    /// mouse-driven pitch delta this tick (radians)
    pub pitch: f32,
    /// keyboard-driven roll (±1.0 from Q/E)
    pub roll: f32,
}
pub type BipedInput = PawnInput;

impl PawnInput {
    pub fn is_valid(&self) -> bool {
        self.forward.is_finite()
            && self.forward.abs() <= 1.0
            && self.right.is_finite()
            && self.right.abs() <= 1.0
            && self.up.is_finite()
            && self.up.abs() <= 1.0
            && self.look_yaw.is_finite()
            && self.look_pitch.is_finite()
            && self.yaw.is_finite()
            && self.yaw.abs() <= 1.0
            && self.pitch.is_finite()
            && self.pitch.abs() <= 1.0
            && self.roll.is_finite()
            && self.roll.abs() <= 1.0
    }
}
