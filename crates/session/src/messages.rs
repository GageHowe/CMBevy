use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::state::state::FreelyMutableState;
#[cfg(feature = "client")]
use common::GameObjectKind;
#[cfg(not(feature = "client"))]
use common::tick::Ticker;
#[cfg(feature = "client")]
use common::tick::{NetworkStats, Ticker};
#[cfg(feature = "client")]
use game_objects::NetworkEntityMap;
#[cfg(feature = "client")]
use game_objects::health::Health;
#[cfg(not(feature = "client"))]
use game_objects::level::LevelBytes;
#[cfg(feature = "client")]
use game_objects::pawn::biped::BipedPawnComponent;
#[cfg(not(feature = "client"))]
use game_objects::pawn::{PendingRespawns, PlayerRegistry};
#[cfg(feature = "client")]
use game_objects::pawn::{Possessed, SeatedInVehicle, WeaponSlots};
#[cfg(feature = "client")]
use game_objects::projectile::{PredictedProjectileMap, ProjectileState};
#[cfg(feature = "client")]
use game_objects::weapon::WeaponState;
#[cfg(feature = "client")]
use game_objects::weapon::helpers as weapon_helpers;
#[cfg(not(feature = "client"))]
use net::message::*;
#[cfg(feature = "client")]
use net::message::{MsgType, NetworkID, SpawnCommand};
#[cfg(feature = "client")]
use net::quic::QuicManager;
#[cfg(not(feature = "client"))]
use net::quic::*;
#[cfg(feature = "client")]
use physics::physics_world::PhysicsWorld;
#[cfg(not(feature = "client"))]
use scripting::ScriptConfig;

#[cfg(feature = "client")]
use crate::helpers::find_networked_entity;
#[cfg(feature = "client")]
use crate::runtime::{ClientSessionState, handle_file_data, handle_map_hash};
#[cfg(not(feature = "client"))]
use crate::{actions::*, connections::*};
use crate::{helpers::drain_inbound, resources::*};

#[cfg(feature = "client")]
pub fn draw_server_state(last: Res<LastServerState>, mut gizmos: Gizmos) {
    let Some(state) = &last.0 else { return };
    for body in state.bodies.values() {
        let pos: Vec3 = body.position.into();
        gizmos.sphere(pos, 0.15, Color::srgb(1.0, 0.2, 0.2));
    }
}

#[cfg(feature = "client")]
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
    drain_inbound(&mut quic, |msg, quic| {
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

#[cfg(feature = "client")]
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
        MsgType::SpawnCommand(cmd) => {
            handle_spawn_command(&mut mp.spawn.commands, &mp.networked, just_spawned, cmd, net_stats.rtt_secs)
        }
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
        ),
        MsgType::WeaponDrop(weapon_id, carrier_id, drop_pos) => handle_weapon_drop(
            &weapon_id,
            &carrier_id,
            drop_pos,
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
            // Skip server weapon state for locally held weapons: cooldown/reload/ammo are
            // predicted client-side, and stale server state (delayed by RTT) causes stutter.
            let is_local_weapon = {
                mp.biped_q
                    .p0()
                    .single()
                    .ok()
                    .map_or(false, |(slots, _)| slots.contains_net_id(&net_id))
            };
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
        MsgType::Scoreboard(snapshot) => gui.scoreboard = Some(snapshot),
        other => warn!("Client: Got unhandled message: {other:?}"),
    }
}

#[cfg(feature = "client")]
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

#[cfg(feature = "client")]
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

#[cfg(feature = "client")]
fn handle_seat_state(
    biped_net_id: &NetworkID,
    vehicle_net_id: Option<&NetworkID>,
    local_net_id: Option<&NetworkID>,
    just_spawned: &JustSpawned,
    networked: &NetworkEntityMap,
    object_kinds: &Query<&GameObjectKind>,
    seated: &Query<&SeatedInVehicle>,
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
                game_objects::messages::push(commands, format!("Entered {kind:?}"));
            }
        }
        None => {
            let old_vehicle = seated.get(biped_entity).ok().map(|seat| seat.0);
            world.set_body_enabled(biped_entity, true);
            commands.entity(biped_entity).remove::<SeatedInVehicle>();
            if local_net_id == Some(biped_net_id)
                && let Some(vehicle_entity) = old_vehicle
                && let Ok(kind) = object_kinds.get(vehicle_entity)
            {
                game_objects::messages::push(commands, format!("Exited {kind:?}"));
            }
        }
    }
}

#[cfg(feature = "client")]
fn handle_despawn(
    net_id: &NetworkID,
    local_net_id: &mut Option<NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
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
                    entity.despawn();
                }
            }
            slots.clear();
        }
        *local_net_id = None;
    } else if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
        slots.remove_by_net_id(net_id);
    }
    if let Ok(mut entity_commands) = commands.get_entity(entity) {
        entity_commands.despawn();
    }
}

#[cfg(feature = "client")]
fn handle_weapon_pickup(
    weapon_id: &NetworkID,
    carrier_net_id: &NetworkID,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    object_kinds: &Query<&GameObjectKind>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Some(weapon_entity) = find_networked_entity(networked, weapon_id) else {
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
                game_objects::messages::push(commands, format!("Picked up {kind:?}"));
            }
        }
        return;
    }
    let carrier = find_networked_entity(networked, carrier_net_id);
    let pivot_e = {
        let q = biped_q.p1();
        carrier.and_then(|entity| q.get(entity).ok().and_then(|b| b.pitch_pivot))
    };
    if let Some(pivot) = pivot_e {
        weapon_helpers::attach_remote_viewmodel(commands, weapon_entity, pivot);
    }
}

#[cfg(feature = "client")]
fn handle_weapon_drop(
    weapon_id: &NetworkID,
    carrier_id: &NetworkID,
    drop_pos: Vec3,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Some(weapon_entity) = find_networked_entity(networked, weapon_id) else {
        return;
    };
    weapon_helpers::place_world_weapon(world, weapon_entity, drop_pos, Vec3::ZERO);
    if local_net_id == Some(carrier_id)
        && let Ok((mut slots, _)) = biped_q.p0().single_mut()
    {
        slots.remove_by_net_id(weapon_id);
    }
    weapon_helpers::detach_viewmodel(commands, world, weapon_entity);
}

#[cfg(feature = "client")]
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

#[cfg(feature = "client")]
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

#[cfg(feature = "client")]
fn handle_weapon_state(
    net_id: &NetworkID,
    state: net::message::WeaponStateSnapshot,
    networked: &NetworkEntityMap,
    weapon_states: &mut Query<&mut WeaponState>,
) {
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let Ok(mut weapon_state) = weapon_states.get_mut(entity) else {
        return;
    };
    weapon_state.apply_snapshot(state);
}

#[cfg(feature = "client")]
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

#[cfg(not(feature = "client"))]
pub fn on_message(
    mut quic: ResMut<QuicManager>,
    script_config: Option<Res<ScriptConfig>>,
    mut active_connections: ResMut<ActiveConnections>,
    mut registry: ResMut<PlayerRegistry>,
    mut pending_connections: ResMut<PendingConnections>,
    mut pending_inputs: ResMut<PendingInputs>,
    mut pending_respawns: ResMut<PendingRespawns>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut sp: ServerMessageParams<'_, '_>,
    tick: Res<Ticker>,
    level_bytes: Option<Res<LevelBytes>>,
) {
    drain_inbound(&mut quic, |msg, quic| {
        process_server_message(
            msg.conn_id,
            msg.msg,
            level_bytes.as_deref(),
            script_config.as_deref(),
            quic,
            &mut active_connections,
            &mut registry,
            &mut pending_connections,
            &mut pending_inputs,
            &mut pending_respawns,
            &mut net_ids,
            &mut sp,
            tick.tick,
        );
    });

    flush_pending_connections(
        &mut quic,
        &mut registry,
        &mut pending_connections,
        &mut net_ids,
        &mut sp,
        tick.tick,
    );
}

#[cfg(not(feature = "client"))]
fn process_server_message(
    conn_id: ConnectionId,
    msg: MsgType,
    level_bytes: Option<&LevelBytes>,
    script_config: Option<&ScriptConfig>,
    quic: &mut QuicManager,
    active_connections: &mut ActiveConnections,
    registry: &mut PlayerRegistry,
    pending_connections: &mut PendingConnections,
    pending_inputs: &mut PendingInputs,
    pending_respawns: &mut PendingRespawns,
    net_ids: &mut NetworkIDResource,
    sp: &mut ServerMessageParams<'_, '_>,
    tick: u64,
) {
    match msg {
        MsgType::Connected => {
            active_connections.0.insert(conn_id);
            info!("GameServer: conn {conn_id} connected; sending map metadata");
            send_connection_files(conn_id, level_bytes, script_config, quic);
        }
        MsgType::RequestMap => send_map_file(conn_id, level_bytes, quic),
        MsgType::ClientReady => {
            info!("GameServer: conn {conn_id} sent ClientReady");
            pending_connections.0.insert(conn_id);
        }
        MsgType::Disconnected => {
            active_connections.0.remove(&conn_id);
            handle_disconnected(
                conn_id,
                pending_respawns,
                registry,
                &sp.pawn_slots,
                &mut sp.held_weapons,
                quic,
                &mut sp.commands,
                &mut sp.world,
            );
            pending_connections.0.remove(&conn_id);
        }
        MsgType::Input(input_seq, kind) => handle_input(conn_id, input_seq, kind, pending_inputs),
        MsgType::FlashlightToggle => {
            handle_flashlight_toggle(conn_id, registry, &mut sp.bipeds, quic);
        }
        MsgType::Interact(target_net_id) => {
            handle_interact(
                conn_id,
                target_net_id,
                registry,
                pending_inputs,
                &sp.all_networked,
                quic,
                &mut sp.world,
                &mut sp.weapon_runtime,
                &mut sp.held_weapons,
                &mut sp.pawn_slots,
                &sp.net_ids,
                &sp.vehicles,
                &mut sp.driver_seats,
                &mut sp.commands,
                &sp.on_pickup_q,
            );
        }
        MsgType::DropWeapon(drop_dir) => handle_drop_weapon(
            conn_id,
            registry,
            &mut sp.pawn_slots,
            &mut sp.held_weapons,
            &mut sp.world,
            &mut sp.weapon_runtime,
            &mut sp.commands,
            quic,
            drop_dir,
        ),
        MsgType::SetActiveWeaponSlot(active_primary) => handle_set_active_weapon_slot(
            conn_id,
            active_primary,
            registry,
            &mut sp.pawn_slots,
            &mut sp.weapon_runtime,
            quic,
        ),
        MsgType::ReloadWeapon(weapon_net_id) => handle_reload_weapon(
            conn_id,
            weapon_net_id,
            registry,
            &sp.all_networked,
            &sp.pawn_slots,
            &mut sp.weapon_runtime,
            quic,
        ),
        MsgType::FireRequest { weapon: weapon_net_id, kind, temp_id, origin, dir } => {
            handle_fire_request(
                conn_id,
                weapon_net_id,
                kind,
                temp_id,
                origin,
                dir,
                registry,
                &sp.all_networked,
                &mut sp.pawn_slots,
                &mut sp.weapon_runtime,
                &mut sp.held_weapons,
                &mut sp.commands,
                &mut sp.world,
                net_ids,
                quic,
                tick,
            )
        }
        MsgType::TimePing(bits) => {
            quic.send(SendTarget::One(conn_id), Channel::Unreliable, &MsgType::TimePong(bits));
        }
        MsgType::Ping(text) => {
            info!("Got a ping from conn_id {:?} with text {}", conn_id, text);
            quic.send(SendTarget::One(conn_id), Channel::Ordered, &MsgType::Pong(text));
        }
        MsgType::ChatMessage(sender, text) => {
            info!("GameServer: Got ChatMessage: [{sender}] {text}");
            quic.send(SendTarget::All, Channel::Ordered, &MsgType::ChatMessage(sender, text));
        }
        other => warn!("Unhandled: {other:?}"),
    }
}
