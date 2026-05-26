use bevy::prelude::*;

#[derive(Component, Default, Reflect)]
pub struct FighterPawnComponent;

pub struct FighterPlugin;

impl Plugin for FighterPlugin {
    fn build(&self, _app: &mut App) {}
}

pub fn spawn_fighter(_entity: Entity, _cmd: &net::message::SpawnCommand, _world: &mut World) {
    panic!("fighter spawn is not wired after the file move")
}
