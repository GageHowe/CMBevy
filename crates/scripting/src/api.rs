//! Registers the small, bounded gameplay API that Lua gametypes can call.

use bevy::{
    ecs::system::{Command, SystemState},
    prelude::*,
};
use common::{GameObjectKind, NetworkID, NetworkIDResource};
use game_objects::{
    SpawnGameObjectCommand, Team,
    bot::{BotController, HeuristicKillerBot},
    health::Health,
    level::{ScriptZone, SpawnPoint, parented_world_pose},
    messages::push_world,
    mode::{MatchPhase, MatchState, PlayerNumbers, TeamNumbers},
    pawn::{PlayerRegistry, WeaponSlots},
};
use mlua::prelude::*;
use net::{
    message::{MsgType, SpawnCommand},
    quic::{Channel, QuicManager, SendTarget},
};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};

use crate::{
    plugin::{PendingWeaponGrants, WeaponGrant},
    runtime::ScriptRuntime,
    tag_index::ScriptTagIndex,
};

fn lua_world(lua: &Lua) -> LuaResult<&mut World> {
    let Some(world_ptr) = lua.app_data_ref::<*mut World>() else {
        return Err(LuaError::runtime("script world unavailable"));
    };
    Ok(unsafe { &mut **world_ptr })
}

fn register_lua_function<F>(lua: &Lua, name: &str, build: F)
where
    F: FnOnce(&Lua) -> LuaResult<LuaFunction>,
{
    let Ok(function) = build(lua) else {
        eprintln!("failed to create Lua function '{name}'");
        return;
    };
    if let Err(err) = lua.globals().set(name, function) {
        eprintln!("failed to register Lua function '{name}': {err}");
    }
}

pub(crate) fn register_script_functions(world: &mut World) {
    let Some(runtime) = world.remove_non_send_resource::<ScriptRuntime>() else {
        return;
    };

    register_lua_function(&runtime.lua, "get_tagged", |lua| {
        lua.create_function(|lua, tag: String| {
            let world = lua_world(lua)?;
            let values = world
                .get_resource::<ScriptTagIndex>()
                .map(|index| {
                    index
                        .get(&tag)
                        .iter()
                        .map(|entity| entity.to_bits() as i64)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            lua.create_sequence_from(values)
        })
    });

    register_lua_function(&runtime.lua, "get_first_tagged", |lua| {
        lua.create_function(|lua, tag: String| {
            let world = lua_world(lua)?;
            Ok(world
                .get_resource::<ScriptTagIndex>()
                .and_then(|index| index.get(&tag).first().copied())
                .map(|entity| entity.to_bits() as i64))
        })
    });

    register_lua_function(&runtime.lua, "entity_has_tag", |lua| {
        lua.create_function(|lua, (entity_id, tag): (i64, String)| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            Ok(world
                .get_resource::<ScriptTagIndex>()
                .is_some_and(|index| index.has(entity, &tag)))
        })
    });

    register_lua_function(&runtime.lua, "entity_exists", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            Ok(world
                .get_entity(Entity::from_bits(entity_id as u64))
                .is_ok())
        })
    });

    register_lua_function(&runtime.lua, "get_entities_in_zone", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            let values = entities_in_zone(world, Entity::from_bits(entity_id as u64))
                .into_iter()
                .map(|entity| entity.to_bits() as i64)
                .collect::<Vec<_>>();
            lua.create_sequence_from(values)
        })
    });

    register_lua_function(&runtime.lua, "is_player", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            Ok(world
                .get_resource::<PlayerRegistry>()
                .is_some_and(|registry| registry.conn_id_for_character(entity).is_some()))
        })
    });

    register_lua_function(&runtime.lua, "is_bot", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            Ok(world
                .get::<BotController>(Entity::from_bits(entity_id as u64))
                .is_some())
        })
    });

    register_lua_function(&runtime.lua, "get_players", |lua| {
        lua.create_function(|lua, ()| {
            let world = lua_world(lua)?;
            let values = world
                .get_resource::<PlayerRegistry>()
                .map(|registry| {
                    registry
                        .character_entities()
                        .map(|entity| entity.to_bits() as i64)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            lua.create_sequence_from(values)
        })
    });

    register_lua_function(&runtime.lua, "get_match_phase", |lua| {
        lua.create_function(|lua, ()| {
            let world = lua_world(lua)?;
            Ok(
                match world.get_resource::<MatchState>().map(|state| state.phase) {
                    Some(MatchPhase::PostGame) => "post_game",
                    _ => "playing",
                },
            )
        })
    });

    register_lua_function(&runtime.lua, "get_match_phase_time", |lua| {
        lua.create_function(|lua, ()| {
            let world = lua_world(lua)?;
            Ok(world
                .get_resource::<MatchState>()
                .map_or(0.0, |state| state.phase_elapsed_secs as f64))
        })
    });

    register_lua_function(&runtime.lua, "end_game", |lua| {
        lua.create_function(|lua, ()| {
            let world = lua_world(lua)?;
            end_game(world, None, None);
            Ok(())
        })
    });

    register_lua_function(&runtime.lua, "end_game_with_player_winner", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            let winner = world
                .get_resource::<PlayerRegistry>()
                .and_then(|registry| registry.conn_id_for_character(entity));
            end_game(world, winner, None);
            Ok(())
        })
    });

    register_lua_function(&runtime.lua, "end_game_with_team_winner", |lua| {
        lua.create_function(|lua, team: i32| {
            let world = lua_world(lua)?;
            end_game(world, None, Some(team.clamp(0, u8::MAX as i32) as u8));
            Ok(())
        })
    });

    register_lua_function(&runtime.lua, "restart_round", |lua| {
        lua.create_function(|lua, ()| {
            let world = lua_world(lua)?;
            if let Some(mut state) = world.get_resource_mut::<MatchState>() {
                state.restart_requested = true;
            }
            Ok(())
        })
    });

    register_lua_function(&runtime.lua, "show_message", |lua| {
        lua.create_function(|lua, text: String| {
            let world = lua_world(lua)?;
            let is_server = world
                .get_resource::<crate::config::ScriptConfig>()
                .is_some_and(|config| config.is_server);
            if is_server {
                if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
                    quic.send(
                        SendTarget::All,
                        Channel::Ordered,
                        &MsgType::OnscreenMessage(text),
                    );
                }
            } else {
                push_world(world, text);
            }
            Ok(())
        })
    });

    register_lua_function(&runtime.lua, "get_player_number", |lua| {
        lua.create_function(|lua, (entity_id, index): (i64, i32)| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            Ok(player_number(world, entity, normalize_index(index)).unwrap_or_default())
        })
    });

    register_lua_function(&runtime.lua, "add_player_number", |lua| {
        lua.create_function(|lua, (entity_id, index, amount): (i64, i32, i32)| {
            let world = lua_world(lua)?;
            add_player_number(
                world,
                Entity::from_bits(entity_id as u64),
                normalize_index(index),
                amount,
            );
            Ok(())
        })
    });

    register_lua_function(&runtime.lua, "get_team_number", |lua| {
        lua.create_function(|lua, (team, index): (i32, i32)| {
            let world = lua_world(lua)?;
            Ok(team_number(
                world,
                team.clamp(0, u8::MAX as i32) as u8,
                normalize_index(index),
            )
            .unwrap_or_default())
        })
    });

    register_lua_function(&runtime.lua, "add_team_number", |lua| {
        lua.create_function(|lua, (team, index, amount): (i32, i32, i32)| {
            let world = lua_world(lua)?;
            add_team_number(
                world,
                team.clamp(0, u8::MAX as i32) as u8,
                normalize_index(index),
                amount,
            );
            Ok(())
        })
    });

    register_lua_function(&runtime.lua, "get_health", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            Ok(world
                .get::<Health>(entity)
                .map(|h| h.current as i32)
                .unwrap_or(0))
        })
    });

    register_lua_function(&runtime.lua, "entity_alive", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            Ok(world
                .get::<Health>(entity)
                .is_some_and(|health| !health.is_dead()))
        })
    });

    register_lua_function(&runtime.lua, "set_health", |lua| {
        lua.create_function(|lua, (entity_id, amount): (i64, i32)| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            if let Some(mut health) = world.get_mut::<Health>(entity) {
                health.current = amount as f32;
            }
            Ok(())
        })
    });

    register_lua_function(&runtime.lua, "spawn_pawn", |lua| {
        lua.create_function(|lua, (team, kind): (i32, Option<String>)| {
            let world = lua_world(lua)?;
            if !world
                .get_resource::<crate::config::ScriptConfig>()
                .is_some_and(|config| config.is_server)
            {
                return Ok(None);
            }
            let kind =
                GameObjectKind::from_name(kind.as_deref().unwrap_or("biped")).filter(|kind| {
                    matches!(
                        kind,
                        GameObjectKind::Biped | GameObjectKind::Spaceship | GameObjectKind::Fighter
                    )
                });
            let Some(kind) = kind else {
                println!("script spawn_pawn failed: invalid kind {kind:?}");
                return Ok(None);
            };
            let team = Team(lua_team(team));
            let Some((pos, rot, vel)) = pick_script_spawn(world, team.0) else {
                println!(
                    "script spawn_pawn failed: no spawn point for team {}",
                    team.0
                );
                return Ok(None);
            };
            let tick = world.resource::<common::tick::Ticker>().tick;
            let net_id = NetworkID(world.resource_mut::<NetworkIDResource>().next());
            let cmd = SpawnCommand {
                net_id,
                position: pos,
                starting_velocity: vel,
                shooter_velocity: Vec3::ZERO,
                rotation: rot,
                server_tick: tick,
                kind,
            };
            let entity = world.spawn_empty().id();
            SpawnGameObjectCommand {
                entity,
                cmd: cmd.clone(),
            }
            .apply(world);
            world.entity_mut(entity).insert(team);
            if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
                quic.send(
                    SendTarget::All,
                    Channel::Ordered,
                    &MsgType::SpawnCommand(cmd),
                );
            }
            Ok(Some(entity.to_bits() as i64))
        })
    });

    register_lua_function(&runtime.lua, "add_bot", |lua| {
        lua.create_function(|lua, (entity_id, brain): (i64, Option<String>)| {
            let world = lua_world(lua)?;
            if !world
                .get_resource::<crate::config::ScriptConfig>()
                .is_some_and(|config| config.is_server)
            {
                return Ok(false);
            }
            if brain.as_deref().is_some_and(|brain| brain != "killer") {
                return Ok(false);
            }
            let entity = Entity::from_bits(entity_id as u64);
            let team = world.get::<Team>(entity).copied().unwrap_or(Team(0));
            world.entity_mut(entity).insert((
                BotController::new(team, HeuristicKillerBot),
                Name::new(format!("Bot (team {})", team.0)),
            ));
            Ok(true)
        })
    });

    register_lua_function(&runtime.lua, "spawn_bot", |lua| {
        lua.create_function(|lua, (team, brain): (i32, Option<String>)| {
            let globals = lua.globals();
            let spawn_pawn: LuaFunction = globals.get("spawn_pawn")?;
            let add_bot: LuaFunction = globals.get("add_bot")?;
            let entity: Option<i64> = spawn_pawn.call((team, "biped"))?;
            if let Some(entity) = entity {
                let _: bool = add_bot.call((entity, brain))?;
            }
            Ok(entity)
        })
    });

    register_lua_function(&runtime.lua, "give_weapon", |lua| {
        lua.create_function(|lua, (owner_id, kind): (i64, String)| {
            let world = lua_world(lua)?;
            if !world
                .get_resource::<crate::config::ScriptConfig>()
                .is_some_and(|config| config.is_server)
            {
                return Ok(false);
            }
            let Some(kind) =
                GameObjectKind::from_name(&kind).filter(game_objects::weapon::is_weapon_kind)
            else {
                return Ok(false);
            };
            let owner = Entity::from_bits(owner_id as u64);
            if world.get::<WeaponSlots>(owner).is_none() {
                println!("script give_weapon failed: owner {owner:?} has no WeaponSlots");
                return Ok(false);
            }
            world
                .resource_mut::<PendingWeaponGrants>()
                .0
                .push(WeaponGrant {
                    owner,
                    kind,
                    weapon: None,
                });
            Ok(true)
        })
    });

    // register_lua_function(&runtime.lua, "spawn_box", |lua| {
    //     lua.create_function(
    //         |lua,
    //          (x, y, z, hx, hy, hz, friction, restitution): (
    //             f64,
    //             f64,
    //             f64,
    //             f64,
    //             f64,
    //             f64,
    //             f64,
    //             f64,
    //         )| {
    //             let world = lua_world(lua)?;
    //             let transform =
    //                 Transform::from_translation(Vec3::new(x as f32, y as f32, z as f32));
    //             let net_id = NetworkID(world.resource_mut::<NetworkIDResource>().next());
    //             let mut state: SystemState<(Commands, ResMut<PhysicsWorld>)> =
    //                 SystemState::new(world);
    //             let (mut commands, mut physics) = state.get_mut(world);
    //             let shape = GenericShape::Primitive(
    //                 ColliderBuilder::cuboid(hx as f32, hy as f32, hz as f32)
    //                     .friction(friction as f32)
    //                     .restitution(restitution as f32),
    //             );
    //             let entity =
    //                 spawn_generic(transform, shape, None, Some(net_id), &mut commands, &mut physics);
    //             state.apply(world);
    //             Ok(entity.to_bits() as i64)
    //         },
    //     )
    // });

    register_lua_function(&runtime.lua, "despawn", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            let mut state: SystemState<Commands> = SystemState::new(world);
            let mut commands = state.get_mut(world);
            commands.entity(entity).despawn();
            state.apply(world);
            Ok(())
        })
    });

    world.insert_non_send_resource(runtime);
}

fn entities_in_zone(world: &mut World, zone_entity: Entity) -> Vec<Entity> {
    let mut state: SystemState<(
        Query<(&ScriptZone, &Transform, Option<&ChildOf>)>,
        Query<&Transform>,
        Query<&ChildOf>,
        Query<&RigidBodyHandleComponent>,
        Res<PhysicsWorld>,
    )> = SystemState::new(world);
    let (zones, parent_transforms, parent_parents, parent_bodies, physics) = state.get(world);
    let Ok((zone, transform, child_of)) = zones.get(zone_entity) else {
        return Vec::new();
    };
    let (position, rotation) = parented_world_pose(
        transform,
        child_of,
        &parent_transforms,
        &parent_parents,
        &parent_bodies,
        &physics,
    );
    physics.entities_intersecting_shape(&zone.shape, 1.0, position, rotation, &[])
}

fn normalize_index(index: i32) -> usize {
    index.max(0) as usize
}

fn lua_team(team: i32) -> u8 {
    team.clamp(0, u8::MAX as i32) as u8
}

fn pick_script_spawn(world: &mut World, team: u8) -> Option<(Vec3, Quat, Vec3)> {
    let tick = world.resource::<common::tick::Ticker>().tick;
    let mut state: SystemState<(
        Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
        Query<&Transform>,
        Query<&ChildOf>,
        Query<&RigidBodyHandleComponent>,
        Res<PhysicsWorld>,
    )> = SystemState::new(world);
    let (spawn_points, parent_transforms, parent_parents, parent_bodies, physics) =
        state.get(world);
    game_objects::lifecycle::pick_spawn_point_with_velocity(
        &spawn_points,
        &parent_transforms,
        &parent_parents,
        &parent_bodies,
        &physics,
        team,
        tick as usize,
    )
}

fn get_number(numbers: &[i32], index: usize) -> i32 {
    numbers.get(index).copied().unwrap_or_default()
}

fn add_number(numbers: &mut Vec<i32>, index: usize, amount: i32) {
    if numbers.len() <= index {
        numbers.resize(index + 1, 0);
    }
    numbers[index] += amount;
}

fn player_number(world: &World, entity: Entity, index: usize) -> Option<i32> {
    let conn_id = world
        .get_resource::<PlayerRegistry>()?
        .conn_id_for_character(entity)?;
    Some(
        world
            .get_resource::<PlayerNumbers>()?
            .0
            .get(&conn_id)
            .map_or(0, |numbers| get_number(numbers, index)),
    )
}

fn add_player_number(world: &mut World, entity: Entity, index: usize, amount: i32) {
    let Some(conn_id) = world
        .get_resource::<PlayerRegistry>()
        .and_then(|registry| registry.conn_id_for_character(entity))
    else {
        return;
    };
    let Some(mut numbers) = world.get_resource_mut::<PlayerNumbers>() else {
        return;
    };
    add_number(numbers.0.entry(conn_id).or_default(), index, amount);
}

fn team_number(world: &World, team: u8, index: usize) -> Option<i32> {
    Some(
        world
            .get_resource::<TeamNumbers>()?
            .0
            .get(&team)
            .map_or(0, |numbers| get_number(numbers, index)),
    )
}

fn add_team_number(world: &mut World, team: u8, index: usize, amount: i32) {
    let Some(mut numbers) = world.get_resource_mut::<TeamNumbers>() else {
        return;
    };
    add_number(numbers.0.entry(team).or_default(), index, amount);
}

fn end_game(world: &mut World, winner_player: Option<u64>, winner_team: Option<u8>) {
    let Some(mut state) = world.get_resource_mut::<MatchState>() else {
        return;
    };
    if state.phase == MatchPhase::PostGame {
        return;
    }
    state.phase = MatchPhase::PostGame;
    state.phase_elapsed_secs = 0.0;
    state.winner_player = winner_player;
    state.winner_team = winner_team;
}
