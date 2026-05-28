//! Shared game object types and the runtime glue that lets the rest of the game spawn them.

use bevy::prelude::*;

pub mod asset_path;
pub mod bot;
pub mod collision;
pub mod components;
pub mod debug_draw;
pub mod flash;
pub mod gc;
pub mod generic;
pub mod health;
pub mod interaction;
pub mod level;
pub mod lifecycle;
pub mod messages;
pub mod mode;
#[path = "../../network_index.rs"]
mod network_index;
#[path = "../pawn/src/mod.rs"]
pub mod pawn;
pub mod projectile;
pub mod reticle;
pub mod shield;
pub mod sound;
mod spawn;
#[cfg(feature = "client")]
pub mod spring_arm;
#[path = "../weapon/mod.rs"]
pub mod weapon;
pub mod zone_effects;

pub use components::{atmosphere, gravity, snap};
pub use generic::{GenericShape, spawn_generic};
pub use mode::{MatchPhase, MatchState, ModeConfig, PlayerNumbers, Team, TeamNumbers};
pub use network_index::NetworkEntityMap;
pub use spawn::{
    CenterOfMassSplashDamage, CollisionSound, DespawnOnDeath, SpawnGameObjectCommand,
    SpawnReplicated, find_entity_by_net_id, insert_spawn_metadata, register_spawnable,
};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthoritySystems;

pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkEntityMap>()
            .init_resource::<spawn::SpawnRegistry>()
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
        #[cfg(feature = "client")]
        sound::configure_collision_sound_system(app);
        app.add_plugins(flash::FlashPlugin);
        app.add_plugins(pawn::PawnPlugin);
        app.add_plugins(projectile::ProjectilePlugin);
        app.add_plugins(shield::ShieldPlugin);
        app.add_plugins(weapon::WeaponPlugin);
        app.add_plugins(zone_effects::ZoneEffectsPlugin);
    }
}

// #[cfg(feature = "client")]
// fn on_remove_networked_entity(
//     event: On<Remove, net::message::NetworkID>,
//     map: Res<NetworkEntityMap>,
//     quic: Option<ResMut<net::quic::QuicManager>>,
// ) {
//     let _ = (event, map, quic);
// }

// #[cfg(not(feature = "client"))]
// fn on_remove_networked_entity(
//     event: On<Remove, net::message::NetworkID>,
//     map: Res<NetworkEntityMap>,
//     mut quic: Option<ResMut<net::quic::QuicManager>>,
// ) {
//     let Some(net_id) = map.get_net_id_for_entity(event.entity).cloned() else {
//         return;
//     };
//     let Some(quic) = quic.as_mut() else {
//         return;
//     };
//     crate::lifecycle::send_despawn_command(quic, net::quic::SendTarget::All, net_id);
// }

/// DO NOT CHANGE - STABLE SYSTEM
/// observer system that automatically detects deleted entities with NetworkID and tells clients to delete them on their end. We should rely on this rather than manually sending despawn messages to the client.
#[allow(unused_variables)]
fn on_remove_networked_entity(
    event: On<Remove, net::message::NetworkID>,
    map: Res<NetworkEntityMap>,
    quic: Option<ResMut<net::quic::QuicManager>>,
) {
    #[cfg(not(feature = "client"))]
    {
        let Some(net_id) = map.get_net_id_for_entity(event.entity).cloned() else {
            return;
        };
        let Some(mut quic) = quic else {
            return;
        };

        // tell all clients to despawn this entity
        quic.send(
            net::quic::SendTarget::All,
            net::quic::Channel::Ordered,
            &net::message::MsgType::DespawnCommand(net_id),
        );
    }
}
