use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_rot};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, Vector3};

use super::super::{AbilityFx, AbilitySpec, BipedAbilityState, drain_meter};

const THRUST: f32 = 0.5;
const DRAIN: f32 = 1.5;

pub const JETPACK: AbilitySpec = AbilitySpec {
    spawn_name: "JetpackAbility",
    meter_max: 100.0,
    meter_regen: 0.4,
    spawn_pickup: spawn_jetpack_pickup,
    apply: apply_jetpack_input,
};

impl crate::archetype::SpawnArchetypeTrait for crate::archetype::JetpackAbility {
    fn spawn(self, entity: Entity, bundle: crate::archetype::SpawnBundle, world: &mut World) {
        spawn_jetpack_pickup(entity, bundle.position, bundle.velocity, world);
        world
            .entity_mut(entity)
            .insert(crate::SpawnReplicated("jetpack"));
        crate::insert_spawn_metadata(entity, world, Some(20.0), true, None, true);
    }
}

pub fn spawn_jetpack_pickup(entity: Entity, pos: Vec3, vel: Vec3, world: &mut World) {
    let rb_handle = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let rb = RigidBodyBuilder::dynamic()
            .translation(pos)
            .linvel(Vector3::new(vel.x, vel.y, vel.z))
            .angular_damping(0.5)
            .build();
        let rb_handle = physics.insert_body(entity, rb);
        let collider = ColliderBuilder::cuboid(0.22, 0.32, 0.14).build();
        let PhysicsWorld {
            collider_set,
            rigid_body_set,
            ..
        } = &mut *physics;
        collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
        rb_handle
    };
    world.entity_mut(entity).insert((
        Transform::from_translation(pos),
        RigidBodyHandleComponent(rb_handle),
        crate::interaction::Interactable { range: 3.0 },
        crate::interaction::InteractionName("Jetpack"),
        super::super::AbilityPickup(&JETPACK),
    ));
    #[cfg(feature = "client")]
    {
        let scene = world
            .resource::<AssetServer>()
            .load("models/placeholder_jetpack.glb#Scene0");
        world
            .entity_mut(entity)
            .insert((WorldAssetRoot(scene), Visibility::default()));
    }
}

pub fn apply_jetpack_input(
    world: &mut PhysicsWorld,
    owner: Entity,
    input: common::BipedInput,
    state: &mut BipedAbilityState,
) -> Option<AbilityFx> {
    let was_active = state.active;
    state.active = input.ability1 && drain_meter(state, DRAIN);
    if !state.active {
        return (was_active != state.active).then_some(AbilityFx::Jetpack(false));
    }
    let Some(&handle) = world.entity_to_handle.get(&owner) else {
        state.active = false;
        return was_active.then_some(AbilityFx::Jetpack(false));
    };
    let (up, mass) = {
        let Some(rb) = world.rigid_body_set.get(handle) else {
            state.active = false;
            return was_active.then_some(AbilityFx::Jetpack(false));
        };
        (rb_rot(rb) * Vec3::Y, rb.mass())
    };
    if let Some(rb) = world.rigid_body_set.get_mut(handle) {
        let impulse = up * THRUST * mass;
        rb.apply_impulse(Vector3::new(impulse.x, impulse.y, impulse.z), true);
    }
    (was_active != state.active).then_some(AbilityFx::Jetpack(true))
}
