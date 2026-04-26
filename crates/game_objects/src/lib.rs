//! Shared game object types and the runtime glue that lets the rest of the game spawn them.

use bevy::prelude::*;
pub use common::GameObjectKind;

pub mod asset_path;
pub mod bot;
pub mod components;
pub mod generic;
pub mod gc;
pub mod health;
pub mod interaction;
pub mod level;
pub mod lifecycle;
pub mod messages;
pub mod mode;
mod network_index;
pub mod pawn;
pub mod projectile;
pub mod sound;
mod spawn;
#[cfg(feature = "client")]
pub mod spring_arm;
pub mod weapon;
pub use components::{atmosphere, gravity, snap};
pub use generic::{GenericShape, spawn_generic};
pub use mode::{MatchPhase, MatchState, ModeConfig, PlayerNumbers, Team, TeamNumbers};
pub use network_index::NetworkEntityMap;
pub use spawn::{
    GameObject, GameObjectRegistry, SpawnGameObjectCommand, dispatch_game_object_on_death,
};

pub struct GameObjectsPlugin;

impl Plugin for GameObjectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkEntityMap>()
            .init_resource::<spawn::GameObjectRegistry>()
            .register_type::<net::message::NetworkID>()
            .register_type::<Team>()
            .init_resource::<messages::GameMessages>()
            .add_observer(on_remove_networked_entity)
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
        app.add_plugins(gc::WorldGcPlugin);
        #[cfg(feature = "client")]
        app.add_plugins(spring_arm::SpringArmPlugin);
        app.add_plugins(pawn::PawnPlugin);
        app.add_plugins(projectile::ProjectilePlugin);
        app.add_plugins(weapon::WeaponPlugin);
    }
}

#[cfg(feature = "client")]
fn on_remove_networked_entity(
    event: On<Remove, net::message::NetworkID>,
    map: Res<NetworkEntityMap>,
    quic: Option<ResMut<net::quic::QuicManager>>,
) {
    let _ = (event, map, quic);
}

#[cfg(not(feature = "client"))]
fn on_remove_networked_entity(
    event: On<Remove, net::message::NetworkID>,
    map: Res<NetworkEntityMap>,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
) {
    let Some(net_id) = map.get_net_id_for_entity(event.entity).cloned() else {
        return;
    };
    let Some(quic) = quic.as_mut() else {
        return;
    };
    quic.send(
        net::quic::SendTarget::All,
        net::quic::Channel::Ordered,
        &net::message::MsgType::DespawnCommand(net_id),
    );
}
