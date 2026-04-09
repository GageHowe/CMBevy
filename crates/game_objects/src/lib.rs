//! Shared game object types and the runtime glue that lets the rest of the game spawn them.

use bevy::prelude::*;
pub use common::GameObjectKind;

pub mod asset_path;
pub mod components;
pub mod generic;
pub mod health;
pub mod interaction;
pub mod level;
pub mod lifecycle;
pub mod messages;
pub mod pawn;
pub mod projectile;
mod network_index;
pub mod score;
pub mod sound;
mod spawn;
pub mod weapon;
pub use components::{atmosphere, planet};
pub use generic::{GenericShape, spawn_generic};
pub use network_index::NetworkEntityMap;
pub use spawn::{GameObject, SpawnGameObjectCommand, dispatch_game_object_on_death};

pub struct GameObjectsPlugin;

impl Plugin for GameObjectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkEntityMap>()
            .register_type::<net::message::NetworkID>()
            .init_resource::<messages::GameMessages>()
            .add_systems(
                PreUpdate,
                (
                    network_index::index_added_network_ids,
                    network_index::index_added_or_changed_rigid_bodies,
                    network_index::index_removed_network_ids,
                    network_index::index_removed_rigid_bodies,
                ),
            )
            .add_systems(
                FixedPreUpdate,
                (
                    network_index::index_added_network_ids,
                    network_index::index_added_or_changed_rigid_bodies,
                    network_index::index_removed_network_ids,
                    network_index::index_removed_rigid_bodies,
                ),
            );
    }
}
