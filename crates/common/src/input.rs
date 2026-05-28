use serde::{Deserialize, Serialize};

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct BipedInput {
    pub forward: f32,
    pub right: f32,
    pub jump: bool,
    pub slide: bool,
    pub ability1: bool,
    pub ability1_pressed: bool,
    pub melee_pressed: bool,
    /// pawn-local yaw from the YawPivot at input time (radians, absolute)
    pub look_yaw: f32,
    /// camera pitch from the PitchPivot at input time (radians, absolute)
    pub look_pitch: f32,
}

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct SpaceshipInput {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub ability1: bool,
    pub ability2: bool,
    /// mouse-driven yaw delta this tick (radians)
    pub yaw: f32,
    /// mouse-driven pitch delta this tick (radians)
    pub pitch: f32,
    /// keyboard-driven roll (±1.0 from Q/E)
    pub roll: f32,
}

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct TruckInput {
    pub throttle: f32,
    pub steer: f32,
    pub brake: f32,
}

#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct HovercarInput {
    pub forward: f32,
    pub right: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

/// Discriminated union of all pawn input types.
/// Serialized directly into MsgType::Input; net layer is transport-only.
/// should this be an enum? We could also give all pawns the same input, just some fields may be unused
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum PawnInputKind {
    Biped(BipedInput),
    Spaceship(SpaceshipInput),
    Truck(TruckInput),
    HovercraftSpaceshipInput(HovercarInput),
}
