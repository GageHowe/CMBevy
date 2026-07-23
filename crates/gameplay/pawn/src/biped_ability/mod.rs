#[cfg(feature = "client")]
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
#[cfg(feature = "client")]
use crate::net::message::NetworkID;
#[cfg(feature = "client")]
use crate::net::quic::{Channel, QuicManager};
#[cfg(not(feature = "client"))]
use crate::net::{
    message::NetworkID,
    quic::{Channel, QuicManager, SendTarget},
};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, Vector3};

#[cfg(not(feature = "client"))]
use crate::SpawnGameObjectCommand;
use crate::pawn::biped::BipedPawnComponent;
#[cfg(feature = "client")]
use crate::pawn::biped::consume_fixed_press;
pub mod fx;
pub mod implementors;
pub use fx::fx_channel;
#[cfg(feature = "client")]
use fx::{cleanup_orphaned_jetpack_fx, sync_jetpack_fx_velocity};
#[cfg(feature = "client")]
pub use fx::{queue_fx, queue_remote_fx};
pub use crate::net::message::AbilityFx;

pub struct BipedAbilityPlugin;
impl Plugin for BipedAbilityPlugin {
    fn build(&self, app: &mut App) {
        crate::register_spawnable(app, "jetpack", implementors::spawn_jetpack);
        crate::register_spawnable(app, "dash", implementors::spawn_dash);
        app.add_systems(
            FixedPreUpdate,
            tick_biped_ability_state.before(super::MovePawnsSet),
        )
        .add_systems(
            FixedUpdate,
            simulate_abilities.in_set(crate::weapon::SimulateWeaponSet),
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

#[derive(Component, Clone, Copy)]
pub struct AbilityPickup(pub &'static AbilitySpec);

#[derive(Clone, Copy, Default)]
pub struct BipedAbilityState {
    pub meter: f32,
    pub active: bool,
}

#[derive(Clone)]
pub struct EquippedAbility {
    pub state: BipedAbilityState,
    spec: &'static AbilitySpec,
}
impl EquippedAbility {
    pub fn new(spec: &'static AbilitySpec) -> Self {
        Self {
            state: BipedAbilityState {
                meter: spec.meter_max,
                active: false,
            },
            spec,
        }
    }

    pub fn status_fraction(&self) -> f32 {
        (self.state.meter / self.spec.meter_max).clamp(0.0, 1.0)
    }

    pub fn spawn_name(&self) -> &'static str {
        self.spec.spawn_name
    }

    /// drop an ability to the ground
    fn drop(self, pos: Vec3, vel: Vec3, world: &mut World) {
        #[cfg(not(feature = "client"))]
        {
            if let Some(net_id) = world
                .get_resource_mut::<common::NetworkIDResource>()
                .map(|mut r| NetworkID(r.next()))
            {
                let entity = world.spawn_empty().id();
                let cmd = crate::net::message::SpawnCommand::new(
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
                        &crate::net::message::MsgType::SpawnCommand(cmd),
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
    pub spawn_pickup: fn(Entity, Vec3, Vec3, &mut World),
    pub apply: fn(
        &mut PhysicsWorld,
        Entity,
        common::BipedInput,
        &mut BipedAbilityState,
    ) -> Option<AbilityFx>,
}

#[derive(Component, Clone, Copy)]
#[component(storage = "SparseSet")]
pub struct AbilityInput(pub common::BipedInput);

fn simulate_abilities(
    mut bipeds: Query<(
        Entity,
        &mut BipedPawnComponent,
        &AbilityInput,
        Option<&NetworkID>,
    )>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    registry: Option<Res<crate::pawn::PlayerRegistry>>,
    mut quic: Option<ResMut<QuicManager>>,
) {
    for (entity, mut biped, input, net_id) in &mut bipeds {
        let fx = apply_input(entity, input.0, &mut world, &mut biped);
        commands.entity(entity).remove::<AbilityInput>();
        let Some(fx) = fx else { continue };
        #[cfg(feature = "client")]
        queue_fx(entity, fx, &world, &mut commands);
        #[cfg(not(feature = "client"))]
        if let (Some(net_id), Some(quic)) = (net_id, quic.as_deref_mut()) {
            let target = registry
                .as_deref()
                .and_then(|registry| registry.conn_id_for_character(entity))
                .map_or(crate::net::quic::SendTarget::All, crate::net::quic::SendTarget::AllExcept);
            quic.send(
                target,
                fx_channel(fx),
                &crate::net::message::MsgType::AbilityFx(net_id.clone(), fx),
            );
        }
        #[cfg(feature = "client")]
        let _ = (&registry, &mut quic, net_id);
    }
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

fn pickup_ability(
    pickup_ability: AbilityPickup,
    biped: Entity,
    pickup: Entity,
    commands: &mut Commands,
) {
    commands.queue(move |world: &mut World| {
        set_ability(biped, EquippedAbility::new(pickup_ability.0), world);
    });
    commands.entity(pickup).despawn();
}

pub(crate) const DROP_SPEED: f32 = 8.0;

/// Drops the equipped ability of `owner`. `throw_vel` is added on top of the owner's physics velocity.
pub(crate) fn drop_owned_ability(owner: Entity, throw_vel: Vec3, world: &mut World) {
    let (pos, vel) = world
        .resource::<PhysicsWorld>()
        .body_drop_pose(owner, throw_vel, Vec3::Y * 1.2)
        .or_else(|| {
            world
                .get::<Transform>(owner)
                .map(|t| (t.translation + Vec3::Y * 1.2, throw_vel))
        })
        .unwrap_or((Vec3::Y * 1.2, throw_vel));
    let Some(ability) = world
        .get_mut::<BipedPawnComponent>(owner)
        .and_then(|mut biped| biped.ability.take())
    else {
        return;
    };
    ability.drop(pos, vel, world);
    sync_ability_state(owner, world);
}

fn set_ability(owner: Entity, ability: EquippedAbility, world: &mut World) {
    if let Some(mut biped) = world.get_mut::<BipedPawnComponent>(owner) {
        biped.ability = Some(ability);
    }
    sync_ability_state(owner, world);
}

fn sync_ability_state(owner: Entity, world: &mut World) {
    #[cfg(not(feature = "client"))]
    {
        let Some(net_id) = world.get::<NetworkID>(owner).cloned() else {
            return;
        };
        let ability = world
            .get::<BipedPawnComponent>(owner)
            .and_then(|biped| biped.ability.as_ref())
            .map(|ability| ability.spawn_name().to_string());
        let Some(conn_id) = world
            .get_resource::<crate::pawn::PlayerRegistry>()
            .and_then(|registry| registry.conn_id_for_character(owner))
        else {
            return;
        };
        if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &crate::net::message::MsgType::AbilityState(net_id, ability),
            );
        }
    }
    #[cfg(feature = "client")]
    let _ = (owner, world);
}

pub fn set_ability_kind(owner: Entity, spawn_name: &str, world: &mut World) -> bool {
    let Some(spec) = ability_spec(spawn_name) else {
        return false;
    };
    let ability = EquippedAbility::new(spec);
    set_ability(owner, ability, world);
    true
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
    set_ability(owner, ability, world);
    true
}

pub fn interact_pickup(
    character: Entity,
    target: Entity,
    world: &PhysicsWorld,
    interactables: &Query<&crate::interaction::Interactable>,
    pickups: &Query<&AbilityPickup>,
    commands: &mut Commands,
) -> bool {
    let Ok(pickup) = pickups.get(target) else {
        return false;
    };
    let Ok(interactable) = interactables.get(target) else {
        return true;
    };
    if !world.entities_within_range(character, target, interactable.range) {
        return true;
    }
    pickup_ability(*pickup, character, target, commands);
    true
}

pub fn handle_drop_request(
    conn_id: crate::net::quic::ConnectionId,
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
pub fn apply_state_message(
    owner_net_id: &NetworkID,
    ability: Option<String>,
    local_net_id: Option<&NetworkID>,
    networked: &crate::NetworkEntityMap,
    commands: &mut Commands,
) {
    if local_net_id != Some(owner_net_id) {
        return;
    }
    let Some(owner) = networked.get_entity(owner_net_id) else {
        return;
    };
    commands.queue(move |world: &mut World| {
        if let Some(ability) = ability {
            let _ = set_ability_kind(owner, &ability, world);
        } else if let Some(mut biped) = world.get_mut::<BipedPawnComponent>(owner) {
            biped.ability = None;
        }
    });
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
            ability.state.meter =
                (ability.state.meter + ability.spec.meter_regen).min(ability.spec.meter_max);
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
    (ability.spec.apply)(world, owner, input, &mut ability.state)
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
                &crate::net::message::MsgType::DropAbility(aim_dir),
            );
        } else {
            commands.queue(DropActiveAbility {
                owner: pawn_entity,
                aim_dir,
            });
        }
    }
}
