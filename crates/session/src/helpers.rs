use bevy::{ecs::system::Commands, prelude::*};
use game_objects::{NetworkEntityMap, pawn::HeldWeaponMap, pawn::WeaponSlots, weapon};
use net::{
    message::{GameObjectKind, NetworkID, NetworkIDResource, WeaponState},
    quic::{Channel, ConnectionId, InboundMessage, QuicManager, SendTarget},
};
use physics::physics_world::PhysicsWorld;

pub(crate) fn find_networked_entity(
    all_networked: &NetworkEntityMap,
    net_id: &NetworkID,
) -> Option<bevy::prelude::Entity> {
    all_networked.get(net_id)
}

pub(crate) fn drain_inbound(
    quic: &mut QuicManager,
    mut handle: impl FnMut(InboundMessage, &mut QuicManager),
) {
    while let Some(msg) = quic.inbound.pop_front() {
        handle(msg, quic);
    }
}

pub(crate) fn fire_weapon_authoritative(
    shooter_entity: Entity,
    weapon_entity: Entity,
    weapon_net_id: &NetworkID,
    kind: GameObjectKind,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    weapon_runtime: &mut Query<(&mut WeaponState, &game_objects::weapon::WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
    quic: Option<&mut QuicManager>,
    tick: u64,
    owner_conn: Option<ConnectionId>,
) -> bool {
    let Some(fired) = weapon::fire_held_weapon(
        shooter_entity,
        weapon_entity,
        weapon_net_id,
        Some(kind),
        temp_id,
        origin,
        dir,
        pawn_slots,
        weapon_runtime,
        held_weapons,
        commands,
        world,
        net_ids,
        tick,
    ) else {
        return false;
    };

    let Some(quic) = quic else {
        return true;
    };
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &net::message::MsgType::WeaponState(fired.weapon_net_id.clone(), fired.weapon_state),
    );
    match owner_conn {
        Some(conn_id) => {
            quic.send(
                SendTarget::AllExcept(conn_id),
                Channel::Unordered,
                &net::message::MsgType::SpawnCommand(fired.fired.spawn_cmd),
            );
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &net::message::MsgType::ProjectileConfirm { temp_id, net_id: fired.fired.net_id },
            );
        }
        None => quic.send(
            SendTarget::All,
            Channel::Unordered,
            &net::message::MsgType::SpawnCommand(fired.fired.spawn_cmd),
        ),
    }
    true
}
