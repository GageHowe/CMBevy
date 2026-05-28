use bevy::{prelude::*, state::state::FreelyMutableState};
use common::tick::{NetworkStats, Ticker};
use gameplay::{
    NetworkEntityMap,
    pawn::{Possessed, WeaponSlots, biped::BipedPawnComponent},
    projectile::{self, ProjectileTempId},
    weapon::helpers as weapon_helpers,
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
    mut world_params: (
        Commands,
        ResMut<PhysicsWorld>,
        ParamSet<(
            Query<(&'static mut WeaponSlots, &'static BipedPawnComponent), With<Possessed>>,
            Query<&'static BipedPawnComponent>,
            Query<&'static mut BipedPawnComponent>,
        )>,
        Res<NetworkEntityMap>,
        Query<Entity, With<Camera3d>>,
        Query<(Entity, &'static ProjectileTempId)>,
        ResMut<projectile::PredictedProjectileMap>,
        Query<&'static gameplay::interaction::InteractionName>,
        Query<&'static gameplay::pawn::biped_ability::OnPickup>,
        Query<&'static gameplay::pawn::Mounted>,
        Query<&'static gameplay::pawn::CharacterMount>,
        Query<&'static Transform>,
        ResMut<PendingWeaponPickups>,
    ),
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
    let (
        ref mut commands,
        ref mut world,
        ref mut biped_q,
        ref networked,
        ref camera,
        ref projectile_q,
        ref mut predicted_projectiles,
        ref interaction_names,
        ref pickup_fns,
        ref mounted,
        ref mounts,
        ref mount_anchor_transforms,
        ref mut pending_weapon_pickups,
    ) = world_params;
    let mut just_spawned: JustSpawned = Default::default();
    let mut local_net_id: Option<NetworkID> = possessed_q.single().ok().map(|(_, nid)| nid.clone());
    crate::helpers::drain_inbound(&mut quic, |msg, quic| {
        net_stats.last_packet_bytes = msg.packet_size;
        process_client_message(
            msg.msg,
            quic,
            &mut gui,
            commands,
            world,
            biped_q,
            networked,
            camera,
            projectile_q,
            predicted_projectiles,
            interaction_names,
            pickup_fns,
            mounted,
            mounts,
            mount_anchor_transforms,
            pending_weapon_pickups,
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

/// central handler for dispatching server->client messages to functions client-side
fn process_client_message<S: States + FreelyMutableState + Copy>(
    msg: MsgType,
    quic: &mut QuicManager,
    gui: &mut GuiState,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    biped_q: &mut ParamSet<(
        Query<(&'static mut WeaponSlots, &'static BipedPawnComponent), With<Possessed>>,
        Query<&'static BipedPawnComponent>,
        Query<&'static mut BipedPawnComponent>,
    )>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    projectile_q: &Query<(Entity, &'static ProjectileTempId)>,
    predicted_projectiles: &mut projectile::PredictedProjectileMap,
    interaction_names: &Query<&'static gameplay::interaction::InteractionName>,
    pickup_fns: &Query<&'static gameplay::pawn::biped_ability::OnPickup>,
    mounted: &Query<&'static gameplay::pawn::Mounted>,
    mounts: &Query<&'static gameplay::pawn::CharacterMount>,
    mount_anchor_transforms: &Query<&'static Transform>,
    pending_weapon_pickups: &mut PendingWeaponPickups,
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
        MsgType::MapHash(hash) => handle_map_hash(hash, quic, commands),
        MsgType::SpawnCommand(cmd) => gameplay::lifecycle::apply_spawn_command(
            commands,
            networked,
            just_spawned,
            cmd,
            net_stats.rtt_secs,
        ),
        MsgType::Possess(net_id) => {
            if let Some(entity) = just_spawned
                .get(&net_id)
                .map(|(entity, _)| *entity)
                .or_else(|| networked.get(&net_id))
                && biped_q.p1().contains(entity)
            {
                local_character.0 = Some(net_id.clone());
            }
            gameplay::lifecycle::apply_possess(
                net_id,
                local_net_id,
                just_spawned,
                networked,
                possessed_q,
                commands,
                ticker,
            );
        }
        MsgType::MountState(biped_net_id, parent_net_id) => {
            gameplay::pawn::mount::apply_mount_state(
                &biped_net_id,
                parent_net_id.as_ref(),
                local_net_id.as_ref(),
                just_spawned,
                networked,
                interaction_names,
                mounted,
                mounts,
                mount_anchor_transforms,
                commands,
                world,
            )
        }
        MsgType::DespawnCommand(net_id) => gameplay::lifecycle::apply_despawn(
            &net_id,
            local_net_id,
            networked,
            camera,
            biped_q,
            commands,
        ),
        MsgType::Disconnected => {
            gameplay::messages::push(commands, "Disconnected.");
            next_state.set(config.main_menu);
        }
        MsgType::WeaponPickup(weapon_id, carrier_net_id) => {
            if !weapon_helpers::apply_pickup_message(
                &weapon_id,
                &carrier_net_id,
                local_net_id.as_ref(),
                networked,
                camera,
                biped_q,
                interaction_names,
                commands,
                world,
            ) {
                pending_weapon_pickups.0.push((weapon_id, carrier_net_id));
            }
        }
        MsgType::PawnLook(net_id, yaw, pitch) => gameplay::pawn::apply_remote_pawn_look(
            &net_id,
            yaw,
            pitch,
            local_net_id.as_ref(),
            networked,
            &mut biped_q.p2(),
        ),
        MsgType::AbilityPickup(carrier_net_id, pickup_net_id) => {
            gameplay::pawn::biped_ability::apply_pickup_message(
                &carrier_net_id,
                &pickup_net_id,
                local_net_id.as_ref(),
                networked,
                pickup_fns,
                commands,
            )
        }
        MsgType::WeaponDrop(weapon_id, carrier_id, drop_pos) => weapon_helpers::apply_drop_message(
            &weapon_id,
            &carrier_id,
            drop_pos,
            net_stats.rtt_secs,
            local_net_id.as_ref(),
            networked,
            biped_q,
            commands,
            world,
        ),
        MsgType::StartBeamCharge(weapon_net_id) => commands.queue(move |world: &mut World| {
            gameplay::weapon::beamer::apply_remote_start_charge(world, weapon_net_id);
        }),
        MsgType::StartBeam {
            weapon: weapon_net_id,
            origin,
            dir,
        } => commands.queue(move |world: &mut World| {
            gameplay::weapon::beamer::apply_remote_start_beam(world, weapon_net_id, origin, dir);
        }),
        MsgType::BeamHitReport {
            weapon: weapon_net_id,
            origin,
            dir,
            ..
        } => commands.queue(move |world: &mut World| {
            gameplay::weapon::beamer::apply_remote_beam_report(world, weapon_net_id, origin, dir);
        }),
        MsgType::EndBeam(weapon_net_id) => commands.queue(move |world: &mut World| {
            gameplay::weapon::beamer::apply_remote_end_beam(world, weapon_net_id);
        }),
        MsgType::HitResult(_, _, _) => {}
        MsgType::ProjectileConfirm { temp_id, net_id } => projectile::confirm_projectile(
            temp_id,
            net_id,
            predicted_projectiles,
            projectile_q,
            commands,
        ),
        MsgType::ProjectileSpawn {
            weapon,
            net_id,
            position,
            starting_velocity,
            shooter_velocity,
        } => commands.queue(move |world: &mut World| {
            gameplay::weapon::spawn_remote_projectile(
                &weapon,
                net_id,
                position,
                starting_velocity,
                shooter_velocity,
                world,
            );
        }),
        MsgType::WeaponState(net_id, weapon_state) => {
            let biped_q = biped_q.p0();
            if biped_q
                .single()
                .ok()
                .is_some_and(|(slots, _)| slots.contains_net_id(&net_id))
            {
                return;
            }
            let Some(entity) = just_spawned
                .get(&net_id)
                .map(|(entity, _)| *entity)
                .or_else(|| networked.get(&net_id))
            else {
                return;
            };
            commands.queue(move |world: &mut World| {
                gameplay::weapon::apply_weapon_state_world(entity, weapon_state, world);
            });
        }
        MsgType::Health(
            net_id,
            current,
            max,
            regen_per_tick_num,
            regen_delay_ticks,
            regen_delay_remaining_ticks,
            regen_accum,
            damage_accum_millis,
        ) => {
            let Some(entity) = just_spawned
                .get(&net_id)
                .map(|(entity, _)| *entity)
                .or_else(|| networked.get(&net_id))
            else {
                return;
            };
            commands.queue(move |world: &mut World| {
                gameplay::health::apply_health(
                    entity,
                    current,
                    max,
                    regen_per_tick_num,
                    regen_delay_ticks,
                    regen_delay_remaining_ticks,
                    regen_accum,
                    damage_accum_millis,
                    world,
                );
            });
        }
        MsgType::Pong(text) => {
            info!("Client: Got PONG \"{text}\"");
            gui.push_log(format!("pong: {text}"));
        }
        MsgType::ChatMessage(sender, text) => gui.push_log(format!("[{sender}] {text}")),
        MsgType::OnscreenMessage(text) => gameplay::messages::push(commands, text),
        MsgType::TimePong(bits) => net_stats.record_pong(bits, time.elapsed_secs_f64()),
        MsgType::State(st) => {
            if st.last_input_seq >= last_acked_input_seq.0 {
                last_acked_input_seq.0 = st.last_input_seq;
                pending.0 = Some(st);
            }
        }
        MsgType::FileData(name, compressed) => handle_file_data(name, compressed, commands),
        MsgType::JetpackFx(net_id, active) => gameplay::pawn::biped_ability::queue_remote_fx(
            &net_id,
            gameplay::pawn::biped_ability::AbilityFx::Jetpack(active),
            local_net_id.as_ref(),
            networked,
            world,
            commands,
        ),
        MsgType::DashFx(net_id, dir) => gameplay::pawn::biped_ability::queue_remote_fx(
            &net_id,
            gameplay::pawn::biped_ability::AbilityFx::Dash(dir),
            local_net_id.as_ref(),
            networked,
            world,
            commands,
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
    interaction_names: Query<&gameplay::interaction::InteractionName>,
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
            &interaction_names,
            &mut commands,
            &mut world,
        ) {
            pending.0.push((weapon_id, carrier_id));
        }
    }
}
