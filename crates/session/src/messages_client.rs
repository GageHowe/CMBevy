use bevy::{prelude::*, state::state::FreelyMutableState};
use common::{
    GameObjectKind,
    tick::{NetworkStats, Ticker},
};
use game_objects::{
    NetworkEntityMap,
    health::Health,
    pawn::{Possessed, SeatedInVehicle, WeaponSlots, biped::BipedPawnComponent},
    projectile::{PredictedProjectileMap, ProjectileState},
    weapon::{WeaponState, helpers as weapon_helpers},
};
use net::{
    message::{MsgType, NetworkID, SpawnCommand},
    quic::QuicManager,
};
use physics::physics_world::{PhysicsWorld, Vector3, rb_pos};

use crate::{
    helpers::find_networked_entity,
    resources::*,
    runtime::{ClientSessionState, handle_file_data, handle_map_hash},
};

pub fn draw_server_state(last: Res<LastServerState>, mut gizmos: Gizmos) {
    let Some(state) = &last.0 else { return };
    for body in state.bodies.values() {
        let pos: Vec3 = body.position.into();
        gizmos.sphere(pos, 0.15, Color::srgb(1.0, 0.2, 0.2));
    }
}

pub(crate) fn on_message<S: States + FreelyMutableState + Copy>(
    quic: Option<ResMut<QuicManager>>,
    mut gui: ResMut<GuiState>,
    mut mp: ClientMessageParams<'_, '_>,
    mut ticker: ResMut<Ticker>,
    mut pending: ResMut<PendingReconciliation>,
    mut net_stats: ResMut<NetworkStats>,
    mut last_acked_input_seq: ResMut<LastAckedInputSeq>,
    time: Res<Time>,
    possessed_q: Query<(Entity, &NetworkID), With<Possessed>>,
    mut next_state: ResMut<NextState<S>>,
    config: Res<ClientSessionState<S>>,
) {
    let Some(mut quic) = quic else {
        return;
    };
    let mut just_spawned: JustSpawned = Default::default();
    let mut local_net_id: Option<NetworkID> = possessed_q.single().ok().map(|(_, nid)| nid.clone());
    crate::helpers::drain_inbound(&mut quic, |msg, quic| {
        net_stats.last_packet_bytes = msg.packet_size;
        process_client_message(
            msg.msg,
            quic,
            &mut gui,
            &mut mp,
            &mut ticker,
            &mut pending,
            &mut net_stats,
            &mut last_acked_input_seq,
            &time,
            &possessed_q,
            &mut next_state,
            &config,
            &mut just_spawned,
            &mut local_net_id,
        );
    });
}

fn process_client_message<S: States + FreelyMutableState + Copy>(
    msg: MsgType,
    quic: &mut QuicManager,
    gui: &mut GuiState,
    mp: &mut ClientMessageParams<'_, '_>,
    ticker: &mut Ticker,
    pending: &mut PendingReconciliation,
    net_stats: &mut NetworkStats,
    last_acked_input_seq: &mut LastAckedInputSeq,
    time: &Time,
    possessed_q: &Query<(Entity, &NetworkID), With<Possessed>>,
    next_state: &mut ResMut<NextState<S>>,
    config: &ClientSessionState<S>,
    just_spawned: &mut JustSpawned,
    local_net_id: &mut Option<NetworkID>,
) {
    match msg {
        MsgType::Connected => {}
        MsgType::MapHash(hash) => handle_map_hash(hash, quic, &mut mp.spawn.commands),
        MsgType::SpawnCommand(cmd) => handle_spawn_command(
            &mut mp.spawn.commands,
            &mp.networked,
            just_spawned,
            cmd,
            net_stats.rtt_secs,
        ),
        MsgType::Possess(net_id) => handle_possess(
            net_id,
            local_net_id,
            just_spawned,
            &mp.networked,
            possessed_q,
            &mut mp.spawn.commands,
            ticker,
        ),
        MsgType::SeatState(biped_net_id, vehicle_net_id) => handle_seat_state(
            &biped_net_id,
            vehicle_net_id.as_ref(),
            local_net_id.as_ref(),
            just_spawned,
            &mp.networked,
            &mp.object_kinds,
            &mp.seated,
            &mp.vehicles,
            &mp.driver_seats,
            &mut mp.spawn.commands,
            &mut mp.world,
        ),
        MsgType::DespawnCommand(net_id) => handle_despawn(
            &net_id,
            local_net_id,
            &mp.networked,
            &mp.camera,
            &mut mp.biped_q,
            &mut mp.spawn.commands,
        ),
        MsgType::Disconnected => {
            game_objects::messages::push(&mut mp.spawn.commands, "Disconnected.");
            next_state.set(config.main_menu);
        }
        MsgType::WeaponPickup(weapon_id, carrier_net_id) => handle_weapon_pickup(
            &weapon_id,
            &carrier_net_id,
            local_net_id.as_ref(),
            &mp.networked,
            &mp.camera,
            &mut mp.biped_q,
            &mp.object_kinds,
            &mut mp.spawn.commands,
            &mut mp.world,
            Some(&mut mp.pending_weapon_pickups),
        ),
        MsgType::BipedLook(net_id, yaw, pitch) => {
            if local_net_id.as_ref() != Some(&net_id)
                && let Some(entity) = find_networked_entity(&mp.networked, &net_id)
                && let Ok(mut biped) = mp.biped_q.p2().get_mut(entity)
            {
                biped.look_yaw = yaw;
                biped.look_pitch = pitch;
            }
        }
        MsgType::AbilityPickup(carrier_net_id, pickup_net_id) => handle_ability_pickup(
            &carrier_net_id,
            &pickup_net_id,
            local_net_id.as_ref(),
            &mp.networked,
            &mp.object_kinds,
            &mut mp.spawn.commands,
        ),
        MsgType::WeaponDrop(weapon_id, carrier_id, drop_pos) => handle_weapon_drop(
            &weapon_id,
            &carrier_id,
            drop_pos,
            net_stats.rtt_secs,
            local_net_id.as_ref(),
            &mp.networked,
            &mut mp.biped_q,
            &mut mp.spawn.commands,
            &mut mp.world,
        ),
        MsgType::HitResult(_, _, _) => {}
        MsgType::ProjectileConfirm { temp_id, net_id } => handle_projectile_confirm(
            temp_id,
            net_id,
            &mut mp.predicted_projectiles,
            &mp.projectile_q,
            &mut mp.spawn.commands,
        ),
        MsgType::HealthUpdate(net_id, current) => {
            handle_health_update(&net_id, current, &mp.networked, &mut mp.health_q)
        }
        MsgType::WeaponState(net_id, state) => {
            let is_local_weapon = mp
                .biped_q
                .p0()
                .single()
                .ok()
                .map_or(false, |(slots, _)| slots.contains_net_id(&net_id));
            if !is_local_weapon {
                handle_weapon_state(&net_id, state, &mp.networked, &mut mp.weapon_states);
            }
        }
        MsgType::Pong(text) => {
            info!("Client: Got PONG \"{text}\"");
            gui.push_log(format!("pong: {text}"));
        }
        MsgType::ChatMessage(sender, text) => gui.push_log(format!("[{sender}] {text}")),
        MsgType::OnscreenMessage(text) => {
            game_objects::messages::push(&mut mp.spawn.commands, text)
        }
        MsgType::TimePong(bits) => net_stats.record_pong(bits, time.elapsed_secs_f64()),
        MsgType::State(st) => {
            if st.last_input_seq >= last_acked_input_seq.0 {
                last_acked_input_seq.0 = st.last_input_seq;
                pending.0 = Some(st);
            }
        }
        MsgType::FileData(name, compressed) => {
            handle_file_data(name, compressed, &mut mp.spawn.commands)
        }
        MsgType::FlashlightState(net_id, on) => handle_flashlight_state(
            &net_id,
            on,
            local_net_id.as_ref(),
            &mp.networked,
            &mp.spawn.entity_children,
            &mut mp.spawn.lights,
        ),
        MsgType::JetpackFx(net_id, active) => game_objects::pawn::biped_ability::queue_remote_fx(
            &net_id,
            game_objects::pawn::biped_ability::AbilityFx::Jetpack(active),
            local_net_id.as_ref(),
            &mp.networked,
            &mp.world,
            &mut mp.spawn.commands,
        ),
        MsgType::DashFx(net_id, dir) => game_objects::pawn::biped_ability::queue_remote_fx(
            &net_id,
            game_objects::pawn::biped_ability::AbilityFx::Dash(dir),
            local_net_id.as_ref(),
            &mp.networked,
            &mp.world,
            &mut mp.spawn.commands,
        ),
        MsgType::Scoreboard(snapshot) => gui.scoreboard = Some(snapshot),
        other => warn!("Client: Got unhandled message: {other:?}"),
    }
}

fn handle_spawn_command(
    commands: &mut Commands,
    networked: &NetworkEntityMap,
    just_spawned: &mut JustSpawned,
    mut cmd: SpawnCommand,
    rtt_secs: f32,
) {
    cmd.position += cmd.starting_velocity * (rtt_secs / 2.0);
    let net_id = cmd.net_id.clone();
    let server_tick = cmd.server_tick;
    let entity = if let Some(entity) = networked.get(&net_id) {
        game_objects::lifecycle::queue_spawn_command_on(entity, cmd, commands);
        entity
    } else {
        let (entity, _, _) = game_objects::lifecycle::queue_spawn_command(cmd, commands);
        entity
    };
    just_spawned.insert(net_id, (entity, server_tick));
}

fn handle_possess(
    net_id: NetworkID,
    local_net_id: &mut Option<NetworkID>,
    just_spawned: &JustSpawned,
    networked: &NetworkEntityMap,
    possessed_q: &Query<(Entity, &NetworkID), With<Possessed>>,
    commands: &mut Commands,
    ticker: &mut Ticker,
) {
    *local_net_id = Some(net_id.clone());
    let result = just_spawned
        .get(&net_id)
        .copied()
        .or_else(|| find_networked_entity(networked, &net_id).map(|entity| (entity, ticker.tick)));
    let Some((entity, server_tick)) = result else {
        return;
    };
    for (old, _) in possessed_q.iter() {
        if old != entity {
            commands.entity(old).remove::<Possessed>();
        }
    }
    ticker.tick = server_tick;
    commands.entity(entity).insert(Possessed::new(128));
}

fn handle_seat_state(
    biped_net_id: &NetworkID,
    vehicle_net_id: Option<&NetworkID>,
    local_net_id: Option<&NetworkID>,
    just_spawned: &JustSpawned,
    networked: &NetworkEntityMap,
    object_kinds: &Query<&GameObjectKind>,
    seated: &Query<&SeatedInVehicle>,
    vehicles: &Query<&game_objects::pawn::vehicle::VehicleComponent>,
    driver_seats: &Query<(&game_objects::pawn::vehicle::DriverSeat, &Transform)>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let biped_entity = just_spawned
        .get(biped_net_id)
        .map(|(entity, _)| *entity)
        .or_else(|| find_networked_entity(networked, biped_net_id));
    let Some(biped_entity) = biped_entity else {
        return;
    };
    match vehicle_net_id.and_then(|id| find_networked_entity(networked, id)) {
        Some(vehicle_entity) => {
            world.set_body_enabled(biped_entity, false);
            commands.entity(biped_entity).insert(SeatedInVehicle(vehicle_entity));
            if local_net_id == Some(biped_net_id)
                && let Ok(kind) = object_kinds.get(vehicle_entity)
            {
                game_objects::messages::push(
                    commands,
                    format!("Entered {}", kind.interaction_name()),
                );
            }
        }
        None => {
            let old_vehicle = seated.get(biped_entity).ok().map(|seat| seat.0);
            let predicted_exit = old_vehicle
                .and_then(|vehicle_entity| {
                    vehicles.get(vehicle_entity).ok().map(|vehicle| (vehicle_entity, vehicle))
                })
                .and_then(|(vehicle_entity, vehicle)| {
                    driver_seats.get(vehicle.driver_seat).ok().and_then(|(seat, seat_transform)| {
                        let exit_offset =
                            seat_transform.rotation * seat.exit_offset + seat_transform.translation;
                        world.predicted_body_point_after(vehicle_entity, exit_offset, 0.0)
                    })
                });
            if let Some((pos, rot, vel, _)) = predicted_exit {
                world.set_body_enabled(biped_entity, true);
                world.set_body_pose(biped_entity, pos, rot, vel, Vec3::ZERO);
            } else {
                world.set_body_enabled(biped_entity, true);
            }
            if let Some(&handle) = world.entity_to_handle.get(&biped_entity)
                && let Some(rb) = world.rigid_body_set.get_mut(handle)
            {
                rb.set_angvel(Vector3::ZERO, true);
            }
            commands.entity(biped_entity).remove::<SeatedInVehicle>();
            if local_net_id == Some(biped_net_id)
                && let Some(vehicle_entity) = old_vehicle
                && let Ok(kind) = object_kinds.get(vehicle_entity)
            {
                game_objects::messages::push(
                    commands,
                    format!("Exited {}", kind.interaction_name()),
                );
            }
        }
    }
}

fn handle_despawn(
    net_id: &NetworkID,
    local_net_id: &mut Option<NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
        Query<&mut BipedPawnComponent>,
    )>,
    commands: &mut Commands,
) {
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let is_local = local_net_id.as_ref() == Some(net_id);
    if is_local {
        if let Ok(cam) = camera.single()
            && let Ok(mut entity) = commands.get_entity(cam)
        {
            entity.remove_parent_in_place();
        }
        if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
            let held: Vec<_> = slots.held_entities().collect();
            for ent in held {
                if let Ok(mut entity) = commands.get_entity(ent) {
                    entity.queue_silenced(|entity: EntityWorldMut| {
                        entity.despawn();
                        Ok::<(), bevy::ecs::world::error::EntityMutableFetchError>(())
                    });
                }
            }
            slots.clear();
        }
        *local_net_id = None;
    } else if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
        slots.remove_by_net_id(net_id);
    }
    if let Ok(mut entity_commands) = commands.get_entity(entity) {
        entity_commands.queue_silenced(|entity: EntityWorldMut| {
            entity.despawn();
            Ok::<(), bevy::ecs::world::error::EntityMutableFetchError>(())
        });
    }
}

fn handle_weapon_pickup(
    weapon_id: &NetworkID,
    carrier_net_id: &NetworkID,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
        Query<&mut BipedPawnComponent>,
    )>,
    object_kinds: &Query<&GameObjectKind>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    mut pending: Option<&mut PendingWeaponPickups>,
) {
    let Some(weapon_entity) = find_networked_entity(networked, weapon_id) else {
        if let Some(pending) = pending.as_mut() {
            pending.0.push((weapon_id.clone(), carrier_net_id.clone()));
        }
        return;
    };
    weapon_helpers::pickup_world_weapon(world, weapon_entity);
    if local_net_id == Some(carrier_net_id) {
        let (slot_result, pivot_e) = if let Ok((mut slots, biped)) = biped_q.p0().single_mut() {
            (slots.assign_pickup(weapon_id.clone(), weapon_entity), biped.pitch_pivot)
        } else {
            (None, None)
        };
        if let Some((is_primary, prev_to_hide)) = slot_result {
            if let Some(prev) = prev_to_hide {
                commands.entity(prev).insert(Visibility::Hidden);
            }
            if let Some(parent) = camera.single().ok().or(pivot_e) {
                weapon_helpers::attach_local_viewmodel(commands, weapon_entity, parent, is_primary);
            }
            if let Ok(kind) = object_kinds.get(weapon_entity) {
                game_objects::messages::push(
                    commands,
                    format!("Picked up {}", kind.interaction_name()),
                );
            }
        }
        return;
    }
    let Some(carrier) = find_networked_entity(networked, carrier_net_id) else {
        if let Some(pending) = pending.as_mut() {
            pending.0.push((weapon_id.clone(), carrier_net_id.clone()));
        }
        return;
    };
    let Some(parent) = ({
        let q = biped_q.p1();
        q.get(carrier).ok().and_then(|b| b.pitch_pivot)
    }) else {
        if let Some(pending) = pending.as_mut() {
            pending.0.push((weapon_id.clone(), carrier_net_id.clone()));
        }
        return;
    };
    weapon_helpers::attach_remote_viewmodel(commands, weapon_entity, parent);
}

pub(crate) fn retry_weapon_pickups(
    mut pending: ResMut<PendingWeaponPickups>,
    networked: Res<NetworkEntityMap>,
    camera: Query<Entity, With<Camera3d>>,
    mut biped_q: ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
        Query<&mut BipedPawnComponent>,
    )>,
    object_kinds: Query<&GameObjectKind>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
) {
    let pickups = std::mem::take(&mut pending.0);
    for (weapon_id, carrier_id) in pickups {
        handle_weapon_pickup(
            &weapon_id,
            &carrier_id,
            None,
            &networked,
            &camera,
            &mut biped_q,
            &object_kinds,
            &mut commands,
            &mut world,
            Some(&mut pending),
        );
    }
}

fn handle_ability_pickup(
    carrier_net_id: &NetworkID,
    pickup_net_id: &NetworkID,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    object_kinds: &Query<&GameObjectKind>,
    commands: &mut Commands,
) {
    if local_net_id != Some(carrier_net_id) {
        return;
    }
    let Some(carrier) = find_networked_entity(networked, carrier_net_id) else {
        return;
    };
    let Some(pickup) = find_networked_entity(networked, pickup_net_id) else {
        return;
    };
    let Ok(kind) = object_kinds.get(pickup) else {
        return;
    };
    let kind = kind.clone();
    commands.queue(move |world: &mut World| {
        let _ = game_objects::pawn::biped_ability::set_ability_kind(carrier, kind, world);
    });
}

fn handle_weapon_drop(
    weapon_id: &NetworkID,
    carrier_id: &NetworkID,
    drop_pos: Vec3,
    rtt_secs: f32,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
        Query<&mut BipedPawnComponent>,
    )>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Some(weapon_entity) = find_networked_entity(networked, weapon_id) else {
        return;
    };
    let Some(carrier_entity) = find_networked_entity(networked, carrier_id) else {
        return;
    };
    let drop_velocity = weapon_helpers::body_velocity(world, carrier_entity);
    let is_local = local_net_id == Some(carrier_id);
    let drop_pos =
        confirmed_drop_pos(world, carrier_entity, drop_pos, drop_velocity, rtt_secs, is_local);
    weapon_helpers::place_world_weapon(world, weapon_entity, drop_pos, drop_velocity);
    if is_local && let Ok((mut slots, _)) = biped_q.p0().single_mut() {
        slots.remove_by_net_id(weapon_id);
        weapon_helpers::set_local_slot_visibility(commands, &slots);
    }
    weapon_helpers::detach_viewmodel(commands, world, weapon_entity);
}

fn confirmed_drop_pos(
    world: &PhysicsWorld,
    carrier_entity: Entity,
    server_pos: Vec3,
    velocity: Vec3,
    rtt_secs: f32,
    is_local: bool,
) -> Vec3 {
    if !is_local {
        return weapon_helpers::predicted_drop_pos(server_pos, velocity, rtt_secs);
    }
    world
        .entity_to_handle
        .get(&carrier_entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(rb_pos)
        .unwrap_or(server_pos)
        + velocity.normalize_or_zero()
}

fn handle_projectile_confirm(
    temp_id: u32,
    net_id: NetworkID,
    predicted_projectiles: &mut PredictedProjectileMap,
    projectile_q: &Query<(Entity, &ProjectileState)>,
    commands: &mut Commands,
) {
    if let Some(projectile_entity) = predicted_projectiles.get(temp_id) {
        predicted_projectiles.remove_temp_id(temp_id);
        if let Ok(mut entity_commands) = commands.get_entity(projectile_entity) {
            entity_commands.insert(net_id);
        }
        return;
    }
    for (projectile_entity, state) in projectile_q.iter() {
        if state.temp_id == temp_id {
            predicted_projectiles.remove_temp_id(temp_id);
            if let Ok(mut entity_commands) = commands.get_entity(projectile_entity) {
                entity_commands.insert(net_id.clone());
            }
            break;
        }
    }
}

fn handle_health_update(
    net_id: &NetworkID,
    current: f32,
    networked: &NetworkEntityMap,
    health_q: &mut Query<&mut Health>,
) {
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let Ok(mut health) = health_q.get_mut(entity) else {
        return;
    };
    health.current = current;
}

fn handle_weapon_state(
    net_id: &NetworkID,
    state: net::message::WeaponState,
    networked: &NetworkEntityMap,
    weapon_states: &mut Query<&mut WeaponState>,
) {
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let Ok(mut weapon_state) = weapon_states.get_mut(entity) else {
        return;
    };
    *weapon_state = state;
}

fn handle_flashlight_state(
    net_id: &NetworkID,
    on: bool,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    entity_children: &Query<&Children>,
    lights: &mut Query<&mut Visibility, With<SpotLight>>,
) {
    if local_net_id == Some(net_id) {
        return;
    }
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let Ok(children) = entity_children.get(entity) else {
        return;
    };
    for child in children.iter() {
        if let Ok(mut vis) = lights.get_mut(child) {
            *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}
