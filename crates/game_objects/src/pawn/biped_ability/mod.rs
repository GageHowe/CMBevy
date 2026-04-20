use std::marker::PhantomData;

use bevy::prelude::*;
use net::message::NetworkID;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_pos, rb_rot};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, Vector3};

#[cfg(feature = "client")]
use crate::pawn::biped::consume_fixed_press;
use crate::{GameObjectKind, pawn::biped::BipedPawnComponent};
pub mod implementors;

type ApplyAbilityInput = fn(&mut PhysicsWorld, Entity, common::BipedInput, &mut BipedAbilityState);
type TickAbilityState = fn(&mut BipedAbilityState);
type PickupAbilityFn = fn(Entity, Entity, &mut Commands);

pub struct BipedAbilityPlugin;
impl Plugin for BipedAbilityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedPreUpdate, tick_biped_ability_state.before(super::MovePawnsSet));
        #[cfg(feature = "client")]
        app.add_systems(
            bevy::app::FixedPreUpdate,
            drop_active_ability_input
                .run_if(resource_exists::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>)
                .in_set(super::GatherInputSet),
        );
    }
}

/// Stored on pickup entities. Called by the interact system when a biped picks it up.
#[derive(Component, Clone, Copy)]
pub struct OnPickup(pub fn(Entity, Entity, &mut Commands));

#[derive(Clone, Copy, Default)]
pub struct BipedAbilityState {
    pub cooldown_ticks: u16,
    pub active_ticks: u16,
    pub meter: f32,
}

#[derive(Clone)]
pub struct EquippedAbility {
    pub state: BipedAbilityState,
    kind: GameObjectKind,
    pickup_radius: f32,
    pickup_color: Color,
    apply_input: ApplyAbilityInput,
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
            tick: A::tick,
            pickup: pickup_ability::<A>,
        }
    }

    fn drop(self, pos: Vec3, vel: Vec3, world: &mut World) {
        let net_id = world
            .get_resource_mut::<common::NetworkIDResource>()
            .map(|mut r| NetworkID(r.next()));
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
    let Some(ability) = world
        .get_mut::<BipedPawnComponent>(owner)
        .and_then(|mut biped| biped.ability.take())
    else {
        return;
    };
    ability.drop(pos, vel, world);
}

pub trait BipedAbility: Default {
    const COOLDOWN_TICKS: u16;
    const ACTIVE_TICKS: u16 = 0;
    const METER_MAX: f32 = 0.0;
    const METER_REGEN: f32 = 0.0;
    const KIND: GameObjectKind;
    const PICKUP_RADIUS: f32 = 0.3;
    const PICKUP_COLOR: (f32, f32, f32) = (0.8, 0.8, 0.8);

    fn initial_state() -> BipedAbilityState {
        BipedAbilityState {
            cooldown_ticks: 0,
            active_ticks: 0,
            meter: Self::METER_MAX,
        }
    }

    fn tick(state: &mut BipedAbilityState) {
        state.cooldown_ticks = state.cooldown_ticks.saturating_sub(1);
        state.active_ticks = state.active_ticks.saturating_sub(1);
        if Self::METER_MAX > 0.0 {
            state.meter = (state.meter + Self::METER_REGEN).min(Self::METER_MAX);
        }
    }

    fn apply_input(
        world: &mut PhysicsWorld,
        owner: Entity,
        input: common::BipedInput,
        state: &mut BipedAbilityState,
    );
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

pub fn start_cooldown(state: &mut BipedAbilityState, cooldown_ticks: u16) {
    state.cooldown_ticks = cooldown_ticks;
}

pub fn can_activate(state: &BipedAbilityState) -> bool {
    state.cooldown_ticks == 0
}

pub fn consume_charge(
    state: &mut BipedAbilityState,
    cooldown_ticks: u16,
    active_ticks: u16,
) -> bool {
    if !can_activate(state) {
        return false;
    }
    state.active_ticks = active_ticks;
    start_cooldown(state, cooldown_ticks);
    true
}

pub fn drain_meter(state: &mut BipedAbilityState, amount: f32) -> bool {
    if state.meter <= 0.0 {
        return false;
    }
    state.meter = (state.meter - amount).max(0.0);
    true
}

pub fn apply_input(
    owner: Entity,
    input: common::BipedInput,
    world: &mut PhysicsWorld,
    biped: &mut BipedPawnComponent,
) {
    let Some(ability) = &mut biped.ability else {
        return;
    };
    (ability.apply_input)(world, owner, input, &mut ability.state);
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
