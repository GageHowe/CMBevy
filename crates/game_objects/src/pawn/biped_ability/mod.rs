use std::marker::PhantomData;

use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::input::gamepad::Gamepad;
#[cfg(feature = "client")]
use net::quic::{Channel, QuicManager};
#[cfg(not(feature = "client"))]
use net::{
    message::{NetworkID, SpawnCommand},
    quic::{Channel, QuicManager, SendTarget},
};
#[cfg(feature = "client")]
use net::message::NetworkID;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_pos, rb_rot};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, Vector3};

#[cfg(not(feature = "client"))]
use crate::SpawnGameObjectCommand;
#[cfg(feature = "client")]
use crate::pawn::biped::consume_fixed_press;
use crate::{GameObjectKind, pawn::biped::BipedPawnComponent, spawn::AppGameObjectExt};
pub mod fx;
pub mod implementors;
pub use fx::{AbilityFx, fx_channel, fx_message};
#[cfg(feature = "client")]
use fx::{cleanup_orphaned_jetpack_fx, sync_jetpack_fx_velocity};
#[cfg(feature = "client")]
pub use fx::{queue_fx, queue_remote_fx};

type TickAbilityState = fn(&mut BipedAbilityState);
type PickupAbilityFn = fn(Entity, Entity, Vec3, &mut Commands);
type AbilityStatusFn = fn(&BipedAbilityState) -> f32;
pub type AbilityInputFn =
    fn(&mut PhysicsWorld, Entity, common::BipedInput, &mut BipedAbilityState) -> Option<AbilityFx>;

pub struct BipedAbilityPlugin;
impl Plugin for BipedAbilityPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<implementors::JetpackPickup>()
            .register_game_object::<implementors::DashPickup>()
            .add_systems(FixedPreUpdate, tick_biped_ability_state.before(super::MovePawnsSet));
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
pub struct OnPickup(pub fn(Entity, Entity, Vec3, &mut Commands));

#[derive(Clone, Copy, Default)]
pub struct BipedAbilityState {
    pub meter: f32,
    pub active: bool,
}

#[derive(Clone)]
pub struct EquippedAbility {
    pub state: BipedAbilityState,
    #[cfg(not(feature = "client"))]
    kind: GameObjectKind,
    apply_input: AbilityInputFn,
    status: AbilityStatusFn,
    tick: TickAbilityState,
    pickup: PickupAbilityFn,
    spawn_pickup: fn(Entity, Vec3, Vec3, &mut World),
}

impl EquippedAbility {
    pub fn new<A: BipedAbility + Send + 'static>() -> Self {
        Self {
            state: A::initial_state(),
            #[cfg(not(feature = "client"))]
            kind: A::KIND,
            apply_input: A::apply_input,
            status: A::status,
            tick: A::tick,
            pickup: pickup_ability::<A>,
            spawn_pickup: A::spawn_pickup,
        }
    }

    pub fn status_fraction(&self) -> f32 {
        (self.status)(&self.state).clamp(0.0, 1.0)
    }

    fn drop(self, pos: Vec3, vel: Vec3, world: &mut World) {
        #[cfg(not(feature = "client"))]
        {
            if let Some(net_id) = world
                .get_resource_mut::<common::NetworkIDResource>()
                .map(|mut r| NetworkID(r.next()))
            {
                let entity = world.spawn_empty().id();
                let cmd = SpawnCommand {
                    net_id,
                    position: pos,
                    starting_velocity: vel,
                    shooter_velocity: Vec3::ZERO,
                    rotation: Quat::IDENTITY,
                    server_tick: world.resource::<common::tick::Ticker>().tick,
                    kind: self.kind.clone(),
                };
                SpawnGameObjectCommand { entity, cmd: cmd.clone() }.apply(world);
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
        (self.spawn_pickup)(entity, pos, vel, world);
        world.entity_mut(entity).insert(OnPickup(self.pickup));
    }
}

/// Spawns a standard ability pickup: dynamic physics body, sphere collider, interactable marker,
/// and (client-only) a placeholder mesh.
pub fn spawn_ability_pickup(
    entity: Entity,
    pos: Vec3,
    vel: Vec3,
    kind: GameObjectKind,
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
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
    world.entity_mut(entity).insert((
        kind.clone(),
        Transform::from_translation(pos),
        RigidBodyHandleComponent(rb_handle),
        crate::interaction::Interactable { range: 3.0 },
    ));
    #[cfg(feature = "client")]
    {
        let color = _color;
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(radius));
        let material = world.resource_mut::<Assets<StandardMaterial>>().add(color);
        world.entity_mut(entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Visibility::default(),
        ));
    }
}

fn pickup_ability<A: BipedAbility + Send + 'static>(
    biped: Entity,
    pickup: Entity,
    aim_dir: Vec3,
    commands: &mut Commands,
) {
    commands.queue(move |world: &mut World| {
        let _ = swap_ability_kind(biped, A::KIND, aim_dir.normalize_or_zero() * DROP_SPEED, world);
    });
    commands.entity(pickup).despawn();
}

fn pickup_callback<A: BipedAbility + Send + 'static>() -> OnPickup {
    OnPickup(pickup_ability::<A>)
}

const DROP_SPEED: f32 = 8.0;

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
            .or_else(|| world.get::<Transform>(owner).map(|t| t.translation + Vec3::Y * 1.2))
            .unwrap_or(Vec3::Y * 1.2);
        (vel + throw_vel, pos)
    };
    let Some(ability) =
        world.get_mut::<BipedPawnComponent>(owner).and_then(|mut biped| biped.ability.take())
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

pub fn set_ability_kind(owner: Entity, kind: GameObjectKind, world: &mut World) -> bool {
    let ability = match kind {
        GameObjectKind::Jetpack => EquippedAbility::new::<implementors::JetpackAbility>(),
        GameObjectKind::Dash => EquippedAbility::new::<implementors::DashAbility>(),
        _ => return false,
    };
    set_ability(owner, ability, world);
    true
}

pub fn swap_ability_kind(
    owner: Entity,
    kind: GameObjectKind,
    throw_vel: Vec3,
    world: &mut World,
) -> bool {
    let ability = match kind {
        GameObjectKind::Jetpack => EquippedAbility::new::<implementors::JetpackAbility>(),
        GameObjectKind::Dash => EquippedAbility::new::<implementors::DashAbility>(),
        _ => return false,
    };
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
    let Ok(&OnPickup(f)) = on_pickup_q.get(target) else {
        return false;
    };
    let Ok(interactable) = interactables.get(target) else {
        return true;
    };
    if !world.entities_within_range(character, target, interactable.range) {
        return true;
    }
    f(character, target, aim_dir, commands);
    quic.send(
        net::quic::SendTarget::One(conn_id),
        Channel::Ordered,
        &net::message::MsgType::AbilityPickup(character_net_id, target_net_id.clone()),
    );
    quic.send(
        net::quic::SendTarget::All,
        Channel::Ordered,
        &net::message::MsgType::DespawnCommand(target_net_id),
    );
    true
}

#[cfg(feature = "client")]
pub fn apply_pickup_message(
    carrier_net_id: &NetworkID,
    pickup_net_id: &NetworkID,
    local_net_id: Option<&NetworkID>,
    networked: &crate::NetworkEntityMap,
    object_kinds: &Query<&GameObjectKind>,
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
    let Ok(kind) = object_kinds.get(pickup) else {
        return;
    };
    let kind = kind.clone();
    commands.queue(move |world: &mut World| {
        let _ = set_ability_kind(carrier, kind, world);
    });
}

pub trait BipedAbility: Default {
    const METER_MAX: f32;
    const METER_REGEN: f32;
    const KIND: GameObjectKind;
    const PICKUP_RADIUS: f32 = 0.3;
    const PICKUP_COLOR: (f32, f32, f32) = (0.8, 0.8, 0.8);

    fn initial_state() -> BipedAbilityState {
        BipedAbilityState { meter: Self::METER_MAX, active: false }
    }

    fn tick(state: &mut BipedAbilityState) {
        state.meter = (state.meter + Self::METER_REGEN).min(Self::METER_MAX);
    }

    fn status(state: &BipedAbilityState) -> f32 {
        state.meter / Self::METER_MAX
    }

    fn spawn_pickup(entity: Entity, pos: Vec3, vel: Vec3, world: &mut World) {
        let (r, g, b) = Self::PICKUP_COLOR;
        spawn_ability_pickup(
            entity,
            pos,
            vel,
            Self::KIND,
            Self::PICKUP_RADIUS,
            Color::srgb(r, g, b),
            world,
        );
    }

    fn apply_input(
        world: &mut PhysicsWorld,
        owner: Entity,
        input: common::BipedInput,
        state: &mut BipedAbilityState,
    ) -> Option<AbilityFx>;
}

#[derive(Component, Reflect)]
#[reflect(Default)]
pub struct AbilityPickup<A: BipedAbility + Reflect + Send + bevy::reflect::TypePath + 'static> {
    #[reflect(ignore)]
    _phantom: PhantomData<A>,
}

impl<A: BipedAbility + Reflect + Send + bevy::reflect::TypePath + 'static> Default
    for AbilityPickup<A>
{
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<A: BipedAbility + Reflect + Send + bevy::reflect::TypePath + 'static> crate::GameObject
    for AbilityPickup<A>
{
    const KIND: GameObjectKind = A::KIND;
    const GC_AFTER_SECS: Option<f32> = Some(20.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        A::spawn_pickup(entity, cmd.position, cmd.starting_velocity, world);
        world.entity_mut(entity).insert((cmd.net_id.clone(), pickup_callback::<A>()));
    }
}

pub struct DropActiveAbility {
    pub owner: Entity,
    pub aim_dir: Vec3,
}

impl bevy::ecs::system::Command for DropActiveAbility {
    fn apply(self, world: &mut World) {
        drop_owned_ability(self.owner, self.aim_dir.normalize_or_zero() * DROP_SPEED, world);
    }
}

pub fn tick_biped_ability_state(mut bipeds: Query<&mut BipedPawnComponent>) {
    for mut biped in &mut bipeds {
        if let Some(ability) = &mut biped.ability {
            (ability.tick)(&mut ability.state);
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
            quic.send_to_server(Channel::Ordered, &net::message::MsgType::DropAbility(aim_dir));
        } else {
            commands.queue(DropActiveAbility { owner: pawn_entity, aim_dir });
        }
    }
}
