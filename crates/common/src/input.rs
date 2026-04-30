use serde::{Deserialize, Serialize};

/// Per-tick input for a biped pawn. Contains movement + look direction.
#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct BipedInput {
    pub forward: f32,
    pub right: f32,
    pub jump: bool,
    pub slide: bool,
    pub ability1: bool,
    pub ability1_pressed: bool,
    /// pawn-local yaw from the YawPivot at input time (radians, absolute)
    pub look_yaw: f32,
    /// camera pitch from the PitchPivot at input time (radians, absolute)
    pub look_pitch: f32,
}

/// Per-tick input for a spaceship pawn. Mouse drives yaw/pitch; Q/E drive roll.
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

/// Per-tick input for a truck pawn.
#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct TruckInput {
    pub throttle: f32,
    pub steer: f32,
    pub brake: f32,
}

/// Per-tick input for a mounted rocket turret pawn.
#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct RocketTurretInput {
    /// Yaw delta for this tick in local turret space.
    pub yaw: f32,
    /// Pitch delta for this tick in local turret space.
    pub pitch: f32,
    /// Fire button held state.
    pub fire: bool,
    /// Rising-edge fire input for one-shot weapons.
    pub fire_pressed: bool,
}

/// Discriminated union of all pawn input types.
/// Serialized directly into MsgType::Input; net layer is transport-only.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum PawnInputKind {
    Biped(BipedInput),
    Spaceship(SpaceshipInput),
    Truck(TruckInput),
    RocketTurret(RocketTurretInput),
}
