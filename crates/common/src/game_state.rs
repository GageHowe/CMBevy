use bevy::prelude::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
pub enum GameState {
    #[default]
    NotPlaying,
    SinglePlayer,
    Multiplayer,
    Editor,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
pub enum SimState {
    #[default]
    Playing,
    Paused,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimulationSystems;
