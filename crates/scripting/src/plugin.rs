//! Wires the scripting runtime into Bevy schedules and keeps the Lua VM hot-reloaded.

use bevy::prelude::*;
use gameplay::{
    health::{PendingPlayerKills, PendingPlayerRemovals, handle_deaths},
    pawn::PlayerRegistry,
};
use mlua::prelude::Lua;

use crate::{
    api::register_script_functions,
    config::ScriptConfig,
    runtime::{ScriptRuntime, call_script, call_script_args, compile_script},
    tag_index::{ScriptTagIndex, sync_script_tags},
};

#[derive(Resource, Default)]
pub(crate) struct PendingWeaponGrants(pub Vec<WeaponGrant>);

pub(crate) struct WeaponGrant {
    pub owner: Entity,
    pub spawn_name: String,
    pub weapon: Option<(Entity, common::NetworkID, net::message::SpawnCommand)>,
}

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send_resource(ScriptRuntime {
            lua: Lua::new(),
            loaded: false,
        })
        .init_resource::<ScriptTagIndex>()
        .init_resource::<PendingWeaponGrants>()
        .init_resource::<PendingPlayerKills>()
        .init_resource::<PendingPlayerRemovals>()
        .add_systems(Startup, (load, register_script_functions).chain())
        .add_systems(PreUpdate, sync_script_tags)
        .add_systems(Update, eval_script_update)
        .add_systems(FixedUpdate, (reload_script, eval_script_fixed_update))
        .add_systems(FixedLast, process_weapon_grants)
        .add_systems(
            FixedUpdate,
            dispatch_player_kill_callbacks.after(handle_deaths),
        );
    }
}

fn load(config: Option<Res<ScriptConfig>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let Some(config) = config else { return };
    compile_script(&config, &mut runtime);
}

/// Recompiles the script whenever `ScriptConfig` is inserted or changed at runtime
/// (e.g. when the client receives the gametype script from the server).
fn reload_script(config: Option<Res<ScriptConfig>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let Some(config) = config else { return };
    if !config.is_changed() {
        return;
    }
    compile_script(&config, &mut runtime);
}

fn eval_script_update(world: &mut World) {
    call_script(world, "on_tick");
}

fn eval_script_fixed_update(world: &mut World) {
    call_script(world, "on_fixed_tick");
}

fn process_weapon_grants(world: &mut World) {
    use gameplay::{
        SpawnGameObjectCommand,
        interaction::Interactable,
        pawn::{HeldWeaponMap, WeaponSlots},
    };
    use net::{
        message::MsgType,
        quic::{Channel, QuicManager, SendTarget},
    };
    use physics::physics_world::{PhysicsWorld, rb_pos};

    let mut grants = world
        .get_resource_mut::<PendingWeaponGrants>()
        .map(|mut pending| std::mem::take(&mut pending.0))
        .unwrap_or_default();
    for mut grant in grants.drain(..) {
        if world.get::<WeaponSlots>(grant.owner).is_none() {
            continue;
        }
        if grant.weapon.is_none() {
            let tick = world.resource::<common::tick::Ticker>().tick;
            let pos = {
                let physics = world.resource::<PhysicsWorld>();
                physics
                    .entity_to_handle
                    .get(&grant.owner)
                    .and_then(|handle| physics.rigid_body_set.get(*handle))
                    .map(rb_pos)
                    .unwrap_or(Vec3::ZERO)
            };
            let weapon_id =
                common::NetworkID(world.resource_mut::<common::NetworkIDResource>().next());
            let spawn_cmd =
                net::message::SpawnCommand::new(weapon_id.clone(), grant.spawn_name.clone(), tick)
                    .position(pos)
                    .rotation(Quat::IDENTITY);
            let weapon_entity = world.spawn_empty().id();
            SpawnGameObjectCommand {
                entity: weapon_entity,
                cmd: spawn_cmd.clone(),
            }
            .apply(world);
            if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
                quic.send(
                    SendTarget::All,
                    Channel::Ordered,
                    &MsgType::SpawnCommand(spawn_cmd.clone()),
                );
            }
            grant.weapon = Some((weapon_entity, weapon_id, spawn_cmd));
            world.resource_mut::<PendingWeaponGrants>().0.push(grant);
            continue;
        }

        let Some((weapon_entity, weapon_id, spawn_cmd)) = grant.weapon.take() else {
            continue;
        };
        #[cfg(not(feature = "client"))]
        let _ = &spawn_cmd;
        #[cfg(feature = "client")]
        let local_parent = if !world
            .get_resource::<QuicManager>()
            .is_some_and(|quic| quic.client_connected)
        {
            let Some(parent) = world
                .get::<gameplay::pawn::biped::BipedPawnComponent>(grant.owner)
                .and_then(|biped| biped.pitch_pivot)
            else {
                grant.weapon = Some((weapon_entity, weapon_id, spawn_cmd));
                world.resource_mut::<PendingWeaponGrants>().0.push(grant);
                continue;
            };
            Some(parent)
        } else {
            None
        };
        let ok = world.resource_scope(|world, mut physics: Mut<PhysicsWorld>| {
            let Some(mut slots) = world.get_mut::<WeaponSlots>(grant.owner) else {
                return false;
            };
            if slots
                .assign_pickup(weapon_id.clone(), weapon_entity)
                .is_none()
            {
                return false;
            }
            physics.set_body_enabled(weapon_entity, false);
            true
        });
        if !ok {
            continue;
        }
        if let Some(mut held) = world.get_resource_mut::<HeldWeaponMap>() {
            held.0.insert(weapon_id.clone(), grant.owner);
        }
        world.entity_mut(weapon_entity).remove::<Interactable>();
        #[cfg(feature = "client")]
        if let Some(parent) = local_parent {
            let mut commands = world.commands();
            gameplay::weapon::helpers::attach_viewmodel(&mut commands, weapon_entity, parent);
        }
        let owner_net_id = world.get::<common::NetworkID>(grant.owner).cloned();
        if let (Some(owner_net_id), Some(mut quic)) =
            (owner_net_id, world.get_resource_mut::<QuicManager>())
        {
            quic.send(
                SendTarget::All,
                Channel::Ordered,
                &MsgType::WeaponPickup(weapon_id, owner_net_id),
            );
        }
    }
}

/// Drains deferred kill callbacks after authoritative death handling so scripts can award
/// numbers or end the game without Rust hard-coding scoring rules.
fn dispatch_player_kill_callbacks(world: &mut World) {
    let kills = world
        .get_resource_mut::<PendingPlayerKills>()
        .map(|mut pending| std::mem::take(&mut pending.0))
        .unwrap_or_default();
    for (victim, killer) in kills {
        call_script_args(
            world,
            "on_player_killed",
            (
                victim.to_bits() as i64,
                killer.map(|entity| entity.to_bits() as i64),
            ),
        );
    }

    let removals = world
        .get_resource_mut::<PendingPlayerRemovals>()
        .map(|mut pending| std::mem::take(&mut pending.0))
        .unwrap_or_default();
    if removals.is_empty() {
        return;
    }
    let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() else {
        return;
    };
    for entity in removals {
        let _ = registry.remove_character(entity);
    }
}
