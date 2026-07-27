use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::*;
use crate::{
    health::{DamageCause, Health, LastDamageSource},
    net::{
        message::{MsgType, NetworkID, OnscreenMessage, WeaponDrop},
        quic::{Channel, QuicManager, SendTarget},
    },
};

pub fn spawn_biped(entity: Entity, cmd: &crate::net::message::SpawnCommand, world: &mut World) {
    let position = cmd.position_or_zero();
    let rotation = cmd.rotation_or_identity();
    let velocity = cmd.velocity_or_zero();
    let angular_velocity = cmd.angular_velocity_or_zero();
    let transform = Transform {
        translation: position,
        rotation,
        ..default()
    };
    world.entity_mut(entity).insert((
        crate::SpawnReplicated("biped"),
        WeaponSlots::new(2),
        Health::new(
            100,
            BIPED_HEALTH_REGEN_PER_SECOND,
            BIPED_HEALTH_REGEN_DELAY_TICKS,
        )
        .with_death(on_biped_death),
        LastDamageSource::default(),
        Transform::from(transform),
        BipedPawnComponent::default(),
    ));

    let (rb_handle, collider_handle) = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let body = RigidBodyBuilder::dynamic()
            .translation(transform.translation)
            .linvel(Vector3::new(velocity.x, velocity.y, velocity.z))
            .lock_rotations()
            .build();
        let rb_handle = physics.insert_body(entity, body);
        if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
            rb.set_rotation(transform.rotation, true);
            rb.set_angvel(
                Vector3::new(angular_velocity.x, angular_velocity.y, angular_velocity.z),
                true,
            );
        }
        let collider =
            movement::make_biped_capsule_collider(CAPSULE_HALF_HEIGHT, MAIN_FRICTION, true);
        let PhysicsWorld {
            collider_set,
            rigid_body_set,
            ..
        } = &mut *physics;
        let collider_handle = collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
        (rb_handle, collider_handle)
    };

    {
        let mut entity_mut = world.entity_mut(entity);
        entity_mut.insert(RigidBodyHandleComponent(rb_handle));
        if let Some(mut biped) = entity_mut.get_mut::<BipedPawnComponent>() {
            biped.collider = Some(collider_handle);
        }
    }

    crate::insert_spawn_metadata(entity, world, None, true, None, true);
    #[cfg(feature = "client")]
    spawn_visuals(entity, world);
}

pub fn on_biped_death(entity: Entity, world: &mut World) {
    #[cfg(feature = "client")]
    if world.get::<Controller>(entity).is_some() {
        super::detach_camera(world);
    }
    let (drop_pos, drop_velocity) = {
        let physics = world.resource::<PhysicsWorld>();
        physics
            .entity_to_handle
            .get(&entity)
            .and_then(|&h| physics.rigid_body_set.get(h))
            .map(|rb| (rb_pos(rb), rb_vel(rb)))
            .unwrap_or((Vec3::ZERO, Vec3::ZERO))
    };
    let held: Vec<Entity> = world
        .get::<WeaponSlots>(entity)
        .map(|slots| slots.held_entities().collect())
        .unwrap_or_default();
    let weapon_drops: Vec<NetworkID> = world
        .get::<WeaponSlots>(entity)
        .map(|slots| slots.held_weapons().map(|(net_id, _)| net_id).collect())
        .unwrap_or_default();
    #[cfg(feature = "client")]
    if let Some(fx_entity) = world
        .get::<BipedPawnComponent>(entity)
        .and_then(|biped| biped.jetpack_fx_entity)
        && let Ok(fx) = world.get_entity_mut(fx_entity)
    {
        fx.despawn();
    }
    let owner_net_id = world.get::<NetworkID>(entity).cloned();
    let last_damage = world
        .get::<LastDamageSource>(entity)
        .copied()
        .unwrap_or_default();
    let killer = last_damage.resolved_attacker();
    let conn_id = world
        .get_resource::<super::PlayerRegistry>()
        .and_then(|registry| registry.conn_id_for_character(entity));
    push_death_message(world, entity, killer, last_damage.cause);
    for weapon_entity in held {
        crate::weapon::helpers::restore_world_weapon(world, weapon_entity, drop_pos, drop_velocity);
    }
    crate::pawn::biped_ability::drop_owned_ability(entity, Vec3::ZERO, world);
    if let Some(mut held_map) = world.get_resource_mut::<super::HeldWeaponMap>() {
        for weapon_id in &weapon_drops {
            held_map.0.remove(weapon_id);
        }
    }
    if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
        for weapon_id in weapon_drops.iter().cloned() {
            if let Some(ref net_id) = owner_net_id {
                quic.send(
                    SendTarget::All,
                    Channel::Ordered,
                    &MsgType::WeaponDrop(WeaponDrop {
                        weapon_id,
                        carrier_net_id: net_id.clone(),
                        drop_pos,
                    }),
                );
            }
        }
    }
    if let Some(mut pending_kills) = world.get_resource_mut::<crate::health::PendingPlayerKills>() {
        pending_kills.0.push((entity, killer));
    }
    let mut deferred_removal = false;
    if let Some(mut pending_removals) =
        world.get_resource_mut::<crate::health::PendingPlayerRemovals>()
    {
        pending_removals.0.push(entity);
        deferred_removal = true;
    }
    if !deferred_removal
        && let Some(mut registry) = world.get_resource_mut::<super::PlayerRegistry>()
    {
        let _ = registry.remove_character(entity);
    }
    if let Some(conn_id) = conn_id {
        let respawn_delay = world
            .get_resource::<crate::mode::ModeConfig>()
            .map_or(common::config::RESPAWN_DELAY_SECS, |cfg| cfg.respawn_delay);
        let team = world
            .get::<crate::Team>(entity)
            .copied()
            .unwrap_or(crate::Team(0));
        if let Some(mut pending_respawns) = world.get_resource_mut::<super::PendingRespawns>() {
            pending_respawns
                .0
                .insert(conn_id, (respawn_delay, "biped".into(), team));
        }
    }
}

#[cfg(feature = "client")]
fn spawn_visuals(entity: Entity, world: &mut World) {
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(bevy::math::primitives::Capsule3d::new(
            CAPSULE_RADIUS,
            CAPSULE_HALF_HEIGHT,
        ));
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(Color::srgb(0.9, 0.4, 0.1));
    world.entity_mut(entity).insert((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Visibility::default(),
    ));
    let pitch_pivot = world
        .spawn((
            PitchPivot { pitch: 0.0 },
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    let yaw_pivot = world
        .spawn((
            YawPivot { yaw: 0.0 },
            Transform::from_translation(VIEW_PIVOT_OFFSET),
            Visibility::default(),
        ))
        .id();
    world.entity_mut(yaw_pivot).add_child(pitch_pivot);
    world.entity_mut(entity).add_child(yaw_pivot);
    world
        .entity_mut(entity)
        .insert(crate::reticle::AimOrigin(pitch_pivot));
    if let Some(mut biped) = world.entity_mut(entity).get_mut::<BipedPawnComponent>() {
        biped.yaw_pivot = Some(yaw_pivot);
        biped.pitch_pivot = Some(pitch_pivot);
    }
}

fn push_death_message(
    world: &mut World,
    victim: Entity,
    killer: Option<Entity>,
    cause: DamageCause,
) {
    let victim_name = player_name(world, victim);
    let killer_name = killer.map(|killer| player_name(world, killer));
    let text = match (killer, killer_name, cause) {
        (Some(killer), Some(_), DamageCause::Explosion) if killer == victim => {
            format!("{victim_name} blew themselves up")
        }
        (Some(killer), Some(_), _) if killer == victim => {
            format!("{victim_name} committed suicide")
        }
        (Some(_), Some(killer_name), DamageCause::Sniper) => {
            format!("{killer_name} sniped {victim_name}")
        }
        (Some(_), Some(killer_name), DamageCause::Explosion) => {
            format!("{killer_name} blew up {victim_name}")
        }
        (Some(_), Some(killer_name), _) => format!("{killer_name} killed {victim_name}"),
        (_, _, DamageCause::Explosion) => format!("{victim_name} blew up"),
        (_, _, DamageCause::Collision) => format!("{victim_name} was flattened"),
        _ => format!("{victim_name} died"),
    };
    #[cfg(not(feature = "client"))]
    if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::OnscreenMessage(OnscreenMessage(text)),
        );
        return;
    }
    crate::messages::push_world(world, text);
}

fn player_name(world: &World, entity: Entity) -> String {
    if let Some(name) = world.get::<Name>(entity) {
        return name.as_str().to_string();
    }
    if let Some(team) = world.get::<crate::Team>(entity)
        && world.get::<crate::bot::BotController>(entity).is_some()
    {
        return format!("Bot (team {})", team.0);
    }
    world
        .get_resource::<super::PlayerRegistry>()
        .and_then(|registry| registry.conn_id_for_character(entity))
        .map(|conn_id| format!("Player {conn_id}"))
        .unwrap_or_else(|| "Player".to_string())
}
