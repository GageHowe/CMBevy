use bevy::{prelude::*, state::state::FreelyMutableState};
use common::{
    GameObjectKind,
    tick::{NetworkStats, Ticker},
};
use game_objects::{
    NetworkEntityMap, health,
    pawn::{Possessed, WeaponSlots, biped::BipedPawnComponent},
    projectile,
    weapon::{self, helpers as weapon_helpers},
};
use net::{
    message::{MsgType, NetworkID},
    quic::QuicManager,
};
use physics::physics_world::PhysicsWorld;

use crate::{
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
    mut local_character: ResMut<LocalCharacterNetId>,
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
            &mut local_character,
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
    local_character: &mut LocalCharacterNetId,
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
        MsgType::SpawnCommand(cmd) => game_objects::lifecycle::apply_spawn_command(
            &mut mp.spawn.commands,
            &mp.networked,
            just_spawned,
            cmd,
            net_stats.rtt_secs,
        ),
        MsgType::Possess(net_id) => {
            if let Some(entity) = just_spawned
                .get(&net_id)
                .map(|(entity, _)| *entity)
                .or_else(|| mp.networked.get(&net_id))
                && mp.object_kinds.get(entity).ok() == Some(&GameObjectKind::Biped)
            {
                local_character.0 = Some(net_id.clone());
            }
            game_objects::lifecycle::apply_possess(
                net_id,
                local_net_id,
                just_spawned,
                &mp.networked,
                possessed_q,
                &mut mp.spawn.commands,
                ticker,
            );
        }
        MsgType::MountState(biped_net_id, parent_net_id) => {
            game_objects::pawn::mount::apply_mount_state(
                &biped_net_id,
                parent_net_id.as_ref(),
                local_net_id.as_ref(),
                just_spawned,
                &mp.networked,
                &mp.object_kinds,
                &mp.mounted,
                &mp.mounts,
                &mp.mount_anchor_transforms,
                &mut mp.spawn.commands,
                &mut mp.world,
            )
        }
        MsgType::DespawnCommand(net_id) => game_objects::lifecycle::apply_despawn(
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
        MsgType::WeaponPickup(weapon_id, carrier_net_id) => {
            if !weapon_helpers::apply_pickup_message(
                &weapon_id,
                &carrier_net_id,
                local_net_id.as_ref(),
                &mp.networked,
                &mp.camera,
                &mut mp.biped_q,
                &mp.object_kinds,
                &mut mp.spawn.commands,
                &mut mp.world,
            ) {
                mp.pending_weapon_pickups
                    .0
                    .push((weapon_id, carrier_net_id));
            }
        }
        MsgType::PawnLook(net_id, yaw, pitch) => game_objects::pawn::apply_remote_pawn_look(
            &net_id,
            yaw,
            pitch,
            local_net_id.as_ref(),
            &mp.networked,
            &mut mp.biped_q.p2(),
            &mut mp.rocket_turrets,
        ),
        MsgType::AbilityPickup(carrier_net_id, pickup_net_id) => {
            game_objects::pawn::biped_ability::apply_pickup_message(
                &carrier_net_id,
                &pickup_net_id,
                local_net_id.as_ref(),
                &mp.networked,
                &mp.object_kinds,
                &mut mp.spawn.commands,
            )
        }
        MsgType::WeaponDrop(weapon_id, carrier_id, drop_pos) => weapon_helpers::apply_drop_message(
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
        MsgType::StartBeamCharge(weapon_net_id) => {
            mp.spawn.commands.queue(move |world: &mut World| {
                game_objects::weapon::beamer::apply_remote_start_charge(world, weapon_net_id);
            })
        }
        MsgType::StartBeam {
            weapon: weapon_net_id,
            origin,
            dir,
        } => mp.spawn.commands.queue(move |world: &mut World| {
            game_objects::weapon::beamer::apply_remote_start_beam(
                world,
                weapon_net_id,
                origin,
                dir,
            );
        }),
        MsgType::BeamHitReport {
            weapon: weapon_net_id,
            origin,
            dir,
            ..
        } => mp.spawn.commands.queue(move |world: &mut World| {
            game_objects::weapon::beamer::apply_remote_beam_report(
                world,
                weapon_net_id,
                origin,
                dir,
            );
        }),
        MsgType::EndBeam(weapon_net_id) => mp.spawn.commands.queue(move |world: &mut World| {
            game_objects::weapon::beamer::apply_remote_end_beam(world, weapon_net_id);
        }),
        MsgType::HitResult(_, _, _) => {}
        MsgType::ProjectileConfirm { temp_id, net_id } => projectile::confirm_projectile(
            temp_id,
            net_id,
            &mut mp.predicted_projectiles,
            &mp.projectile_q,
            &mut mp.spawn.commands,
        ),
        MsgType::HealthUpdate(net_id, current) => {
            health::apply_health_update(&net_id, current, &mp.networked, &mut mp.health_q)
        }
        MsgType::WeaponState(net_id, state) => {
            let is_local_weapon = mp
                .biped_q
                .p0()
                .single()
                .ok()
                .map_or(false, |(slots, _)| slots.contains_net_id(&net_id));
            if !is_local_weapon {
                weapon::apply_weapon_state(&net_id, state, &mp.networked, &mut mp.weapon_states);
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
        if !weapon_helpers::apply_pickup_message(
            &weapon_id,
            &carrier_id,
            None,
            &networked,
            &camera,
            &mut biped_q,
            &object_kinds,
            &mut commands,
            &mut world,
        ) {
            pending.0.push((weapon_id, carrier_id));
        }
    }
}
