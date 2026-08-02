use bevy::prelude::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
pub enum GameState {
    #[default]
    NotPlaying,
    SinglePlayer,
    Multiplayer,
    Editor,
}

/// on server, this is Paused if there are no players. It gates SimulationSystems.
/// on client in Singleplayer, does the same thing if paused.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
pub enum SimState {
    #[default]
    Playing,
    Paused,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimulationSystems;
