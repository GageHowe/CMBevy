//! Registers the small, bounded gameplay API that Lua gametypes can call.

use crate::runtime::ScriptRuntime;
use crate::tag_index::ScriptTagIndex;
use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use common::{NetworkID, NetworkIDResource};
use game_objects::health::Health;
use game_objects::level::{ScriptZone, parented_world_pose};
use game_objects::pawn::PlayerRegistry;
use game_objects::score::PlayerScores;
use game_objects::{GenericShape, spawn_generic};
use mlua::prelude::*;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
use rapier3d::prelude::ColliderBuilder;

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
        error!("failed to create Lua function '{name}'");
        return;
    };
    if let Err(err) = lua.globals().set(name, function) {
        error!("failed to register Lua function '{name}': {err}");
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
                .is_some_and(|registry| registry.conn_id_for_entity(entity).is_some()))
        })
    });

    register_lua_function(&runtime.lua, "get_player_score", |lua| {
        lua.create_function(|lua, entity_id: i64| {
            let world = lua_world(lua)?;
            let entity = Entity::from_bits(entity_id as u64);
            Ok(player_score(world, entity).unwrap_or_default())
        })
    });

    register_lua_function(&runtime.lua, "add_player_score", |lua| {
        lua.create_function(|lua, (entity_id, amount): (i64, i32)| {
            let world = lua_world(lua)?;
            add_player_score(world, Entity::from_bits(entity_id as u64), amount);
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
    physics.entities_intersecting_shape(&zone.shape, zone.scale.max(0.001), position, rotation, &[])
}

fn player_score(world: &World, entity: Entity) -> Option<i32> {
    let conn_id = world
        .get_resource::<PlayerRegistry>()?
        .conn_id_for_entity(entity)?;
    Some(
        world
            .get_resource::<PlayerScores>()?
            .0
            .get(&conn_id)
            .copied()
            .unwrap_or_default(),
    )
}

fn add_player_score(world: &mut World, entity: Entity, amount: i32) {
    let Some(conn_id) = world
        .get_resource::<PlayerRegistry>()
        .and_then(|registry| registry.conn_id_for_entity(entity))
    else {
        return;
    };
    let Some(mut scores) = world.get_resource_mut::<PlayerScores>() else {
        return;
    };
    *scores.0.entry(conn_id).or_default() += amount;
}
