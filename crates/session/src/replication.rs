use bevy::prelude::*;
use common::tick::Ticker;
use game_objects::{
    health::Health,
    lifecycle::spawn_game_object,
    mode::ModeConfig,
    pawn::{HeldWeaponMap, PlayerRegistry, WeaponSlots},
    *,
};
use net::{message::*, quic::*};
use physics::physics_world::*;

use crate::resources::*;

pub(super) fn spawn_player(
    conn_id: ConnectionId,
    kind: GameObjectKind,
    team: Team,
    spawn_pos: Vec3,
    spawn_rot: Quat,
    spawn_vel: Vec3,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    net_ids: &mut NetworkIDResource,
    commands: &mut Commands,
    tick: u64,
) {
    let kind_debug = format!("{kind:?}");
    let (entity, net_id, spawn_cmd) = spawn_game_object(
        kind, spawn_pos, spawn_rot, spawn_vel, tick, commands, net_ids,
    );
    commands.entity(entity).insert(team);

    for other_conn_id in registry.controlled_conn_ids() {
        quic.send(
            SendTarget::One(other_conn_id),
            Channel::Ordered,
            &MsgType::SpawnCommand(spawn_cmd.clone()),
        );
    }
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::SpawnCommand(spawn_cmd),
    );
    game_objects::pawn::possess_pawn(conn_id, entity, &net_id, registry, quic);
    registry.register_character(conn_id, entity, net_id);
    info!("GameServer: spawned {kind_debug} for conn {conn_id}");
}

pub(super) fn slots_to_held(slots: &Option<&WeaponSlots>) -> Vec<(NetworkID, Entity)> {
    let Some(slots) = slots else {
        return vec![];
    };
    slots.held_weapons().collect()
}

pub(super) fn kill_player(
    entity: Entity,
    net_id: NetworkID,
    held_weapons: Vec<(NetworkID, Entity)>,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    held_weapon_map: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let (drop_pos, drop_velocity) = world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| (rb_pos(rb), rb_vel(rb)))
        .unwrap_or((Vec3::ZERO, Vec3::ZERO));
    for (wid, weapon_entity) in held_weapons {
        held_weapon_map.0.remove(&wid);
        game_objects::weapon::helpers::place_world_weapon(
            world,
            weapon_entity,
            drop_pos,
            drop_velocity,
        );
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::WeaponDrop(wid, net_id.clone(), drop_pos),
        );
    }
    registry.remove_character(entity);
    commands.entity(entity).despawn();
    let _ = net_id;
}

/// Broadcasts authoritative health changes for any networked entity whose health changed this frame.
pub fn broadcast_health_updates(
    mut quic: ResMut<QuicManager>,
    health_q: Query<(&Health, &NetworkID), Changed<Health>>,
) {
    for (health, net_id) in health_q.iter() {
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::HealthUpdate(net_id.clone(), health.current),
        );
    }
}

/// Broadcasts the authoritative physics snapshot and any dirty pawn look state for the current tick.
pub fn broadcast_tick(
    mut quic: ResMut<QuicManager>,
    tick: Res<Ticker>,
    world: Res<PhysicsWorld>,
    query: Query<(&NetworkID, &RigidBodyHandleComponent)>,
    mut biped_looks: Query<(
        &NetworkID,
        &mut game_objects::pawn::biped::BipedPawnComponent,
    )>,
    mut rocket_turret_looks: Query<(
        &NetworkID,
        &mut game_objects::pawn::RocketTurretPawnComponent,
    )>,
    registry: Res<PlayerRegistry>,
    last_input_seq: Res<LastProcessedInputSeq>,
    mut history: ResMut<BodyHistory>,
) {
    let state = snapshot_bodies(&world, tick.tick, query.iter());
    history.0.insert(tick.tick, state.clone());
    history.0.retain(|&t, _| tick.tick.saturating_sub(t) <= 128);
    for conn_id in registry.controlled_conn_ids() {
        let mut state_for_client = state.clone();
        state_for_client.last_input_seq = *last_input_seq.0.get(&conn_id).unwrap_or(&0);
        quic.send(
            SendTarget::One(conn_id),
            Channel::Unreliable,
            &MsgType::State(state_for_client),
        );
    }
    game_objects::pawn::broadcast_dirty_look_updates(
        &mut quic,
        &mut biped_looks,
        &mut rocket_turret_looks,
    );
}

/// Broadcasts a compact scoreboard snapshot at a lower frequency than the main physics tick.
pub fn broadcast_scoreboard(
    mut quic: ResMut<QuicManager>,
    tick: Res<Ticker>,
    registry: Res<PlayerRegistry>,
    teams_q: Query<&Team>,
    player_numbers: Res<game_objects::mode::PlayerNumbers>,
    team_numbers: Res<game_objects::mode::TeamNumbers>,
    mode: Option<Res<ModeConfig>>,
) {
    if tick.tick % 15 != 0 {
        return;
    }
    let mode = mode.map_or_else(ModeConfig::default, |value| value.clone());
    let mut players = registry
        .character_entries()
        .map(|(conn_id, (entity, net_id))| ScoreboardEntry {
            net_id: net_id.clone(),
            label: format!("Player {conn_id}"),
            team: teams_q.get(*entity).map_or(0, |team| team.0),
            value: player_numbers
                .0
                .get(conn_id)
                .and_then(|numbers| numbers.get(mode.leaderboard_number_index))
                .copied()
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    players.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));

    let mut teams = (0..mode.team_count.max(1))
        .map(|team| ScoreboardEntry {
            net_id: NetworkID(0),
            label: format!("Team {}", team + 1),
            team,
            value: team_numbers
                .0
                .get(&team)
                .and_then(|numbers| numbers.get(mode.leaderboard_number_index))
                .copied()
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    teams.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));

    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::Scoreboard(ScoreboardSnapshot {
            teams_enabled: mode.teams_enabled,
            scoring: mode.scoring,
            leaderboard_scope: mode.leaderboard_scope,
            time_limit_secs: mode.time_limit_secs,
            leaderboard_label: mode.leaderboard_label,
            primary_objective_label: mode.primary_objective_label,
            players,
            teams,
        }),
    );
}
