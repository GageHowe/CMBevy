#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, bevy::prelude::States, Default)]
pub enum GameState {
    #[default]
    MainMenu,
    SinglePlayer,
    Multiplayer,
    Editor,
}
