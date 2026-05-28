#[cfg(feature = "client")]
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
#[cfg(feature = "client")]
use net::message::NetworkID;
#[cfg(feature = "client")]
use net::quic::{Channel, QuicManager};
#[cfg(not(feature = "client"))]
use net::{
    message::NetworkID,
    quic::{Channel, QuicManager, SendTarget},
};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_pos, rb_rot};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, Vector3};

#[cfg(not(feature = "client"))]
use crate::SpawnGameObjectCommand;
#[cfg(feature = "client")]
use crate::pawn::biped::consume_fixed_press;
use crate::pawn::biped::BipedPawnComponent;
pub mod fx;
pub mod implementors;
pub use fx::{AbilityFx, fx_channel, fx_message};
#[cfg(feature = "client")]
use fx::{cleanup_orphaned_jetpack_fx, sync_jetpack_fx_velocity};
#[cfg(feature = "client")]
pub use fx::{queue_fx, queue_remote_fx};

pub type AbilityInputFn =
    fn(&mut PhysicsWorld, Entity, common::BipedInput, &mut BipedAbilityState) -> Option<AbilityFx>;

pub struct BipedAbilityPlugin;
impl Plugin for BipedAbilityPlugin {
    fn build(&self, app: &mut App) {
        crate::register_spawnable(app, "jetpack", spawn_jetpack_pickup);
        crate::register_spawnable(app, "dash", spawn_dash_pickup);
        app.add_systems(
            FixedPreUpdate,
            tick_biped_ability_state.before(super::MovePawnsSet),
        );
        #[cfg(feature = "client")]
        app.add_systems(
            bevy::app::FixedPreUpdate,
            drop_active_ability_input
                .run_if(resource_exists::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>)
                .in_set(super::GatherInputSet),
        )
        .add_systems(bevy::app::FixedPostUpdate, sync_jetpack_fx_velocity)
        .add_systems(Update, cleanup_orphaned_jetpack_fx);
    }
}

/// Stored on pickup entities. Called by the interact system when a biped picks it up.
#[derive(Component, Clone, Copy)]
pub struct OnPickup(pub fn(Entity, &mut World) -> bool);

#[derive(Clone, Copy, Default)]
pub struct BipedAbilityState {
    pub meter: f32,
    pub active: bool,
}

#[derive(Clone)]
pub struct EquippedAbility {
    pub state: BipedAbilityState,
    spec: &'static AbilitySpec,
    apply_input: AbilityInputFn,
}

impl EquippedAbility {
    pub fn new(spec: &'static AbilitySpec) -> Self {
        Self {
            state: BipedAbilityState {
                meter: spec.meter_max,
                active: false,
            },
            spec,
            apply_input: spec.apply_input,
        }
    }

    pub fn status_fraction(&self) -> f32 {
        (self.state.meter / self.spec.meter_max).clamp(0.0, 1.0)
    }

    fn drop(self, pos: Vec3, vel: Vec3, world: &mut World) {
        #[cfg(not(feature = "client"))]
        {
            if let Some(net_id) = world
                .get_resource_mut::<common::NetworkIDResource>()
                .map(|mut r| NetworkID(r.next()))
            {
                let entity = world.spawn_empty().id();
                let cmd = net::message::SpawnCommand::new(
                    net_id,
                    self.spec.spawn_name,
                    world.resource::<common::tick::Ticker>().tick,
                )
                .position(pos)
                .rotation(Quat::IDENTITY)
                .velocity(vel);
                SpawnGameObjectCommand {
                    entity,
                    cmd: cmd.clone(),
                }
                .apply(world);
                if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
                    quic.send(
                        SendTarget::All,
                        Channel::Ordered,
                        &net::message::MsgType::SpawnCommand(cmd),
                    );
                }
                return;
            }
        }

        let entity = world.spawn_empty().id();
        (self.spec.spawn_pickup)(entity, pos, vel, world);
    }
}

pub struct AbilitySpec {
    pub spawn_name: &'static str,
    pub meter_max: f32,
    pub meter_regen: f32,
    pub apply_input: AbilityInputFn,
    pub spawn_pickup: fn(Entity, Vec3, Vec3, &mut World),
}

/// Spawns a standard ability pickup: dynamic physics body, sphere collider, interactable marker,
/// and (client-only) a placeholder mesh.
pub fn spawn_ability_pickup(
    entity: Entity,
    pos: Vec3,
    vel: Vec3,
    radius: f32,
    _color: Color,
    world: &mut World,
) {
    let rb_handle = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let rb = RigidBodyBuilder::dynamic()
            .translation(pos)
            .linvel(Vector3::new(vel.x, vel.y, vel.z))
            .angular_damping(0.5)
            .build();
        let rb_handle = physics.insert_body(entity, rb);
        rb_handle
    };
    let collider = ColliderBuilder::ball(radius).build();
    let mut physics = world.resource_mut::<PhysicsWorld>();
    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *physics;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
    world.entity_mut(entity).insert((
        Transform::from_translation(pos),
        RigidBodyHandleComponent(rb_handle),
        crate::interaction::Interactable { range: 3.0 },
    ));
    #[cfg(feature = "client")]
    {
        let color = _color;
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Sphere::new(radius));
        let material = world.resource_mut::<Assets<StandardMaterial>>().add(color);
        world.entity_mut(entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Visibility::default(),
        ));
    }
}

fn pickup_ability(pickup_behavior: OnPickup, biped: Entity, pickup: Entity, commands: &mut Commands) {
    commands.queue(move |world: &mut World| {
        let _ = pickup_behavior.0(biped, world);
    });
    commands.entity(pickup).despawn();
}

pub(crate) const DROP_SPEED: f32 = 8.0;

/// Drops the equipped ability of `owner`. `throw_vel` is added on top of the owner's physics velocity.
fn drop_owned_ability(owner: Entity, throw_vel: Vec3, world: &mut World) {
    let (vel, pos) = {
        let physics = world.resource::<PhysicsWorld>();
        let forward = throw_vel.normalize_or_zero();
        let vel = physics
            .entity_to_handle
            .get(&owner)
            .and_then(|h| physics.rigid_body_set.get(*h))
            .map(|rb| {
                let v = rb.linvel();
                Vec3::new(v.x, v.y, v.z)
            })
            .unwrap_or(Vec3::ZERO);
        let pos = physics
            .entity_to_handle
            .get(&owner)
            .and_then(|h| physics.rigid_body_set.get(*h))
            .map(|rb| rb_pos(rb) + rb_rot(rb) * Vec3::Y * 1.2 + forward)
            .or_else(|| {
                world
                    .get::<Transform>(owner)
                    .map(|t| t.translation + Vec3::Y * 1.2)
            })
            .unwrap_or(Vec3::Y * 1.2);
        (vel + throw_vel, pos)
    };
    let Some(ability) = world
        .get_mut::<BipedPawnComponent>(owner)
        .and_then(|mut biped| biped.ability.take())
    else {
        return;
    };
    ability.drop(pos, vel, world);
}

fn set_ability(owner: Entity, ability: EquippedAbility, world: &mut World) {
    if let Some(mut biped) = world.get_mut::<BipedPawnComponent>(owner) {
        biped.ability = Some(ability);
    }
}

pub fn set_ability_kind(owner: Entity, spawn_name: &str, world: &mut World) -> bool {
    let Some(spec) = ability_spec(spawn_name) else {
        return false;
    };
    let ability = EquippedAbility::new(spec);
    set_ability(owner, ability, world);
    true
}

pub fn equip_jetpack(owner: Entity, world: &mut World) -> bool {
    set_ability_kind(owner, "jetpack", world)
}

pub fn equip_dash(owner: Entity, world: &mut World) -> bool {
    set_ability_kind(owner, "dash", world)
}

pub fn swap_ability_kind(
    owner: Entity,
    spawn_name: &str,
    throw_vel: Vec3,
    world: &mut World,
) -> bool {
    let Some(spec) = ability_spec(spawn_name) else {
        return false;
    };
    let ability = EquippedAbility::new(spec);
    drop_owned_ability(owner, throw_vel, world);
    if let Some(mut biped) = world.get_mut::<BipedPawnComponent>(owner) {
        biped.ability = Some(ability);
    }
    true
}

pub fn drop_ability_on_death(owner: Entity, world: &mut World) {
    drop_owned_ability(owner, Vec3::ZERO, world);
}

pub fn interact_pickup(
    conn_id: net::quic::ConnectionId,
    character: Entity,
    character_net_id: NetworkID,
    target: Entity,
    target_net_id: NetworkID,
    world: &PhysicsWorld,
    interactables: &Query<&crate::interaction::Interactable>,
    on_pickup_q: &Query<&OnPickup>,
    commands: &mut Commands,
    quic: &mut QuicManager,
    aim_dir: Vec3,
) -> bool {
    let Ok(pickup) = on_pickup_q.get(target) else {
        return false;
    };
    let Ok(interactable) = interactables.get(target) else {
        return true;
    };
    if !world.entities_within_range(character, target, interactable.range) {
        return true;
    }
    let _ = aim_dir;
    pickup_ability(*pickup, character, target, commands);
    quic.send(
        net::quic::SendTarget::One(conn_id),
        Channel::Ordered,
        &net::message::MsgType::AbilityPickup(character_net_id, target_net_id.clone()),
    );
    crate::lifecycle::send_despawn_command(quic, net::quic::SendTarget::All, target_net_id);
    true
}

pub fn handle_drop_request(
    conn_id: net::quic::ConnectionId,
    registry: &crate::pawn::PlayerRegistry,
    commands: &mut Commands,
    drop_dir: Vec3,
) {
    let Some((player_entity, _)) = registry.character(conn_id) else {
        return;
    };
    commands.queue(DropActiveAbility {
        owner: player_entity,
        aim_dir: drop_dir,
    });
}

#[cfg(feature = "client")]
pub fn apply_pickup_message(
    carrier_net_id: &NetworkID,
    pickup_net_id: &NetworkID,
    local_net_id: Option<&NetworkID>,
    networked: &crate::NetworkEntityMap,
    pickup_q: &Query<&OnPickup>,
    commands: &mut Commands,
) {
    if local_net_id != Some(carrier_net_id) {
        return;
    }
    let Some(carrier) = networked.get_entity(carrier_net_id) else {
        return;
    };
    let Some(pickup) = networked.get_entity(pickup_net_id) else {
        return;
    };
    let Ok(&pickup) = pickup_q.get(pickup) else {
        return;
    };
    commands.queue(move |world: &mut World| {
        let _ = pickup.0(carrier, world);
    });
}

pub fn spawn_jetpack_pickup(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
    implementors::spawn_jetpack_pickup(entity, cmd.position_or_zero(), cmd.velocity_or_zero(), world);
    world.entity_mut(entity).insert(crate::SpawnReplicated("jetpack"));
    crate::insert_spawn_metadata(entity, world, Some(20.0), true, None, true);
}

pub fn spawn_dash_pickup(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
    implementors::spawn_dash_pickup(entity, cmd.position_or_zero(), cmd.velocity_or_zero(), world);
    world.entity_mut(entity).insert(crate::SpawnReplicated("dash"));
    crate::insert_spawn_metadata(entity, world, Some(20.0), true, None, true);
}

pub struct DropActiveAbility {
    pub owner: Entity,
    pub aim_dir: Vec3,
}

impl bevy::ecs::system::Command for DropActiveAbility {
    fn apply(self, world: &mut World) {
        drop_owned_ability(
            self.owner,
            self.aim_dir.normalize_or_zero() * DROP_SPEED,
            world,
        );
    }
}

pub fn tick_biped_ability_state(mut bipeds: Query<&mut BipedPawnComponent>) {
    for mut biped in &mut bipeds {
        if let Some(ability) = &mut biped.ability {
            ability.state.meter = (ability.state.meter + ability.spec.meter_regen).min(ability.spec.meter_max);
        }
    }
}

pub fn drain_meter(state: &mut BipedAbilityState, amount: f32) -> bool {
    if state.meter < amount {
        return false;
    }
    state.meter -= amount;
    true
}

pub fn apply_input(
    owner: Entity,
    input: common::BipedInput,
    world: &mut PhysicsWorld,
    biped: &mut BipedPawnComponent,
) -> Option<AbilityFx> {
    let Some(ability) = &mut biped.ability else {
        return None;
    };
    (ability.apply_input)(world, owner, input, &mut ability.state)
}

fn ability_spec(spawn_name: &str) -> Option<&'static AbilitySpec> {
    [implementors::JETPACK, implementors::DASH]
        .iter()
        .find(|spec| spec.spawn_name == spawn_name)
}

#[cfg(feature = "client")]
fn drop_active_ability_input(
    keyboard: Res<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>,
    mouse: Res<bevy::input::ButtonInput<bevy::input::mouse::MouseButton>>,
    gamepads: Query<&Gamepad>,
    egui_wants: Option<Res<bevy_egui::input::EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    state: Res<State<common::game_state::GameState>>,
    possessed: Query<Entity, With<super::Possessed>>,
    pitch_pivots: Query<&GlobalTransform, With<super::PitchPivot>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut commands: Commands,
    mut quic: Option<ResMut<QuicManager>>,
    mut drop_pressed_latched: Local<bool>,
) {
    let blocked = egui_wants.map_or(false, |e| e.wants_any_input());
    let Ok(pawn_entity) = possessed.single() else {
        return;
    };
    let Ok(gt) = pitch_pivots.single() else {
        return;
    };
    let (_, pivot_rot, _) = gt.to_scale_rotation_translation();
    let aim_dir = camera
        .single()
        .ok()
        .map(|gt| gt.compute_transform().rotation * Vec3::NEG_Z)
        .unwrap_or(pivot_rot * Vec3::NEG_Z);
    let drop_pressed = !blocked
        && consume_fixed_press(
            bindings.pressed(
                common::InputAction::DropAbility,
                &keyboard,
                &mouse,
                common::active_gamepad(gamepads.iter()),
            ),
            &mut drop_pressed_latched,
        );

    if drop_pressed {
        if matches!(state.get(), common::game_state::GameState::Multiplayer)
            && let Some(quic) = quic.as_mut()
        {
            quic.send_to_server(
                Channel::Ordered,
                &net::message::MsgType::DropAbility(aim_dir),
            );
        } else {
            commands.queue(DropActiveAbility {
                owner: pawn_entity,
                aim_dir,
            });
        }
    }
}
