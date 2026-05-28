#[allow(unused_imports)]
use std::collections::VecDeque;

use bevy::prelude::*;

pub const MESSAGE_TTL_SECS: f64 = 6.0;

#[cfg(feature = "client")]
#[derive(Clone)]
pub struct GameMessage {
    pub text: String,
    pub created_at: f64,
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
pub struct GameMessages(pub VecDeque<GameMessage>);

// why is this differenciation necessary?
#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub struct GameMessages;

pub fn push(commands: &mut Commands, text: impl Into<String>) {
    let text = text.into();
    commands.queue(move |world: &mut World| push_world(world, text));
}

pub fn push_world(_world: &mut World, text: impl Into<String>) {
    let text = text.into();
    #[cfg(feature = "client")]
    {
        let now = _world.resource::<Time>().elapsed_secs_f64();
        let mut messages = _world.resource_mut::<GameMessages>();
        messages.0.push_back(GameMessage {
            text,
            created_at: now,
        });

        // clear excessive messages
        if messages.0.len() > 16 {
            messages.0.remove(0);
        }
    }

    // on server debug mode, do debug prints
    #[cfg(all(debug_assertions, not(feature = "client")))]
    {
        eprintln!("{text}");
    }
}
