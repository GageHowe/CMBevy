use std::marker::PhantomData;

use bevy::prelude::*;
use net::message::NetworkID;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_pos, rb_rot};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, Vector3};

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
type PickupAbilityFn = fn(Entity, Entity, &mut Commands);
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
pub struct OnPickup(pub fn(Entity, Entity, &mut Commands));

#[derive(Clone, Copy, Default)]
pub struct BipedAbilityState {
    pub meter: f32,
    pub active: bool,
}

#[derive(Clone)]
pub struct EquippedAbility {
    pub state: BipedAbilityState,
    kind: GameObjectKind,
    pickup_radius: f32,
    pickup_color: Color,
    apply_input: AbilityInputFn,
    status: AbilityStatusFn,
    tick: TickAbilityState,
    pickup: PickupAbilityFn,
}

impl EquippedAbility {
    pub fn new<A: BipedAbility + Send + 'static>() -> Self {
        Self {
            state: A::initial_state(),
            kind: A::KIND,
            pickup_radius: A::PICKUP_RADIUS,
            pickup_color: Color::srgb(A::PICKUP_COLOR.0, A::PICKUP_COLOR.1, A::PICKUP_COLOR.2),
            apply_input: A::apply_input,
            status: A::status,
            tick: A::tick,
            pickup: pickup_ability::<A>,
        }
    }

    pub fn status_fraction(&self) -> f32 {
        (self.status)(&self.state).clamp(0.0, 1.0)
    }

    fn drop(self, pos: Vec3, vel: Vec3, world: &mut World) {
        let net_id =
            world.get_resource_mut::<common::NetworkIDResource>().map(|mut r| NetworkID(r.next()));
        let entity = world.spawn_empty().id();
        if let Some(net_id) = net_id {
            world.entity_mut(entity).insert(net_id);
        }
        spawn_ability_pickup(
            entity,
            pos,
            vel,
            self.kind,
            self.pickup_radius,
            self.pickup_color,
            world,
        );
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
        let col = ColliderBuilder::ball(radius).build();
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
        collider_set.insert_with_parent(col, rb_handle, rigid_body_set);
        rb_handle
    };
    world.entity_mut(entity).insert((
        kind,
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
    commands: &mut Commands,
) {
    commands.queue(AttachAbility::<A>::new(biped));
    commands.entity(pickup).despawn();
}

fn pickup_callback<A: BipedAbility + Send + 'static>() -> OnPickup {
    OnPickup(pickup_ability::<A>)
}

fn attach_ability_to_biped<A: BipedAbility + Send + 'static>(owner: Entity, world: &mut World) {
    drop_owned_ability(owner, Vec3::ZERO, world);
    if let Some(mut biped) = world.get_mut::<BipedPawnComponent>(owner) {
        biped.ability = Some(EquippedAbility::new::<A>());
    }
}

/// Drops the equipped ability of `owner`. `throw_vel` is added on top of the owner's physics velocity.
fn drop_owned_ability(owner: Entity, throw_vel: Vec3, world: &mut World) {
    let (vel, pos) = {
        let physics = world.resource::<PhysicsWorld>();
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
            .map(|rb| rb_pos(rb) + rb_rot(rb) * Vec3::Y * 1.2)
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

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let (r, g, b) = A::PICKUP_COLOR;
        spawn_ability_pickup(
            entity,
            cmd.position,
            cmd.starting_velocity,
            A::KIND,
            A::PICKUP_RADIUS,
            Color::srgb(r, g, b),
            world,
        );
        world.entity_mut(entity).insert(pickup_callback::<A>());
    }
}

pub struct AttachAbility<A: BipedAbility + Send> {
    pub owner: Entity,
    _phantom: PhantomData<A>,
}

impl<A: BipedAbility + Send> AttachAbility<A> {
    pub fn new(owner: Entity) -> Self {
        Self { owner, _phantom: PhantomData }
    }
}

impl<A: BipedAbility + Send + 'static> bevy::ecs::system::Command for AttachAbility<A> {
    fn apply(self, world: &mut World) {
        attach_ability_to_biped::<A>(self.owner, world);
    }
}

pub struct DropActiveAbility {
    pub owner: Entity,
    pub aim_dir: Vec3,
}

impl bevy::ecs::system::Command for DropActiveAbility {
    fn apply(self, world: &mut World) {
        drop_owned_ability(self.owner, self.aim_dir * 5.0, world);
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
    egui_wants: Option<Res<bevy_egui::input::EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    possessed: Query<Entity, With<super::Possessed>>,
    pitch_pivots: Query<&GlobalTransform, With<super::PitchPivot>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut commands: Commands,
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
            bindings.pressed(common::InputAction::DropAbility, &keyboard, &mouse),
            &mut drop_pressed_latched,
        );

    if drop_pressed {
        commands.queue(DropActiveAbility { owner: pawn_entity, aim_dir });
    }
}
