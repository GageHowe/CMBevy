#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, bevy::prelude::States, Default)]
pub enum GameState {
    #[default]
    NotPlaying,
    SinglePlayer,
    Multiplayer,
    Editor,
}
