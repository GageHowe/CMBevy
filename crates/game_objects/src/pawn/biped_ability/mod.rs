use std::marker::PhantomData;

#[cfg(feature = "client")]
use bevy::ecs::system::{In, SystemId};
use bevy::prelude::*;
use net::message::NetworkID;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, Vector3};

use super::{BipedPawnComponent, CameraEffector};
use crate::{GameObjectKind, sound::SoundQueue};
pub mod implementors;

pub struct BipedAbilityPlugin;
impl Plugin for BipedAbilityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, tick_biped_ability_state);
        #[cfg(feature = "client")]
        app.add_systems(
            bevy::app::FixedPreUpdate,
            drive_biped_abilities
                .run_if(resource_exists::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>)
                .in_set(super::GatherInputSet),
        );
    }
}

/// Links an ability entity back to the biped that owns it.
#[derive(Component, Clone, Copy)]
pub struct AbilityOwner(pub Entity);

#[derive(Component)]
pub struct BipedAbilityComponent;

#[derive(Component, Clone, Reflect)]
pub struct BipedAbilityConfig {
    pub cooldown_ticks: u16,
    pub active_ticks: u16,
    /// Maximum meter value. 0.0 means no meter system.
    pub meter_max: f32,
    /// Meter restored per fixed tick when not draining.
    pub meter_regen: f32,
}

#[derive(Component, Clone, Copy, Reflect, Default)]
pub struct BipedAbilityState {
    pub cooldown_ticks: u16,
    pub active_ticks: u16,
    /// Current meter level. Starts full (== config.meter_max).
    pub meter: f32,
}

impl BipedAbilityState {
    pub fn new<A: BipedAbility>() -> Self {
        Self { cooldown_ticks: 0, active_ticks: 0, meter: A::METER_MAX }
    }
}

impl BipedAbilityConfig {
    pub fn new<A: BipedAbility>() -> Self {
        Self {
            cooldown_ticks: A::COOLDOWN_TICKS,
            active_ticks: A::ACTIVE_TICKS,
            meter_max: A::METER_MAX,
            meter_regen: A::METER_REGEN,
        }
    }
}

#[cfg(feature = "client")]
#[derive(Component, Clone, Copy)]
pub struct BipedAbilityDriver {
    pub fixed_update: SystemId<In<BipedAbilityInput>>,
}

#[cfg(feature = "client")]
#[derive(Clone, Copy)]
pub struct BipedAbilityInput {
    pub ability: Entity,
    pub pressed: bool,
    pub held: bool,
    pub alt_pressed: bool,
    pub alt_held: bool,
    pub origin: Vec3,
    pub owner: Entity,
    pub tick: u64,
}

pub struct BipedAbilityCtx<'a> {
    pub ability: Entity,
    pub pressed: bool,
    pub held: bool,
    pub alt_pressed: bool,
    pub alt_held: bool,
    pub origin: Vec3,
    pub aim_dir: Vec3,
    pub owner: Entity,
    pub owner_biped: Option<&'a BipedPawnComponent>,
    pub tick: u64,
    pub net_id: Option<&'a NetworkID>,
    pub owner_net_id: Option<&'a NetworkID>,
    pub sound: Option<&'a mut SoundQueue>,
    pub camera: Option<&'a mut CameraEffector>,
    pub quic: Option<&'a mut net::quic::QuicManager>,
    pub predicted: Option<&'a mut common::PredictedCommands>,
    pub state: &'a mut BipedAbilityState,
    pub config: BipedAbilityConfig,
}

/// Spawns a standard ability pickup: dynamic physics body, sphere collider, interactable marker,
/// and (client-only) a placeholder mesh.  Call from `GameObject::spawn` on your pickup type.
pub fn spawn_ability_pickup(
    entity: Entity,
    cmd: &net::message::SpawnCommand,
    kind: crate::GameObjectKind,
    radius: f32,
    color: Color,
    world: &mut World,
) {
    let rb_handle = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let rb = RigidBodyBuilder::dynamic()
            .translation(cmd.position)
            .linvel(Vector3::new(
                cmd.starting_velocity.x,
                cmd.starting_velocity.y,
                cmd.starting_velocity.z,
            ))
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
        Transform::from_translation(cmd.position),
        RigidBodyHandleComponent(rb_handle),
        crate::interaction::Interactable { range: 3.0 },
    ));
    #[cfg(feature = "client")]
    {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(radius));
        let material = world.resource_mut::<Assets<StandardMaterial>>().add(color);
        world.entity_mut(entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Visibility::default(),
        ));
    }
}

pub trait BipedAbility: Component<Mutability = bevy::ecs::component::Mutable> + Default {
    const MODEL_PATH: &'static str;
    const ICON_PATH: &'static str;
    const COOLDOWN_TICKS: u16;
    const ACTIVE_TICKS: u16 = 0;
    /// Max meter capacity. 0.0 disables the meter system for this ability.
    const METER_MAX: f32 = 0.0;
    /// Meter restored per fixed tick (auto-regen). 0.0 = no regen.
    const METER_REGEN: f32 = 0.0;
    /// `GameObjectKind` used for the world-placed pickup entity.
    const KIND: GameObjectKind;
    /// Sphere radius for the pickup collider and placeholder mesh.
    const PICKUP_RADIUS: f32 = 0.3;
    /// Placeholder mesh color as linear (r, g, b).
    const PICKUP_COLOR: (f32, f32, f32) = (0.8, 0.8, 0.8);

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut BipedAbilityCtx,
    );
}

// ── Generic pickup / attach ───────────────────────────────────────────────────

/// World-placed pickup entity for any `BipedAbility`.
/// Interacting with it despawns it and attaches the ability to the player.
#[derive(Component, Reflect)]
#[reflect(Default)]
pub struct AbilityPickup<A: BipedAbility + Reflect + bevy::reflect::TypePath + 'static> {
    #[reflect(ignore)]
    _phantom: PhantomData<A>,
}

impl<A: BipedAbility + Reflect + bevy::reflect::TypePath + 'static> Default for AbilityPickup<A> {
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<A: BipedAbility + Reflect + bevy::reflect::TypePath + 'static> crate::GameObject
    for AbilityPickup<A>
{
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let (r, g, b) = A::PICKUP_COLOR;
        spawn_ability_pickup(entity, cmd, A::KIND, A::PICKUP_RADIUS, Color::srgb(r, g, b), world);
    }
}

/// Command that spawns a `BipedAbility` entity owned by `owner`.
pub struct AttachAbility<A: BipedAbility> {
    pub owner: Entity,
    _phantom: PhantomData<A>,
}

impl<A: BipedAbility> AttachAbility<A> {
    pub fn new(owner: Entity) -> Self {
        Self { owner, _phantom: PhantomData }
    }
}

impl<A: BipedAbility + 'static> bevy::ecs::system::Command for AttachAbility<A> {
    fn apply(self, world: &mut World) {
        let bundle = biped_ability_bundle(A::default(), world);
        world.spawn((bundle, AbilityOwner(self.owner)));
    }
}

#[cfg(feature = "client")]
pub fn biped_ability_bundle<A: BipedAbility + 'static>(
    ability: A,
    world: &mut World,
) -> impl Bundle {
    (
        BipedAbilityComponent,
        ability,
        BipedAbilityConfig::new::<A>(),
        BipedAbilityState::new::<A>(),
        BipedAbilityDriver { fixed_update: world.register_system_cached(use_biped_ability::<A>) },
    )
}

#[cfg(not(feature = "client"))]
pub fn biped_ability_bundle<A: BipedAbility>(ability: A, _world: &mut World) -> impl Bundle {
    (BipedAbilityComponent, ability, BipedAbilityConfig::new::<A>(), BipedAbilityState::new::<A>())
}

#[cfg(feature = "client")]
pub fn use_biped_ability<A: BipedAbility>(
    In(input): In<BipedAbilityInput>,
    mut abilities: Query<(&mut A, &mut BipedAbilityState, &BipedAbilityConfig)>,
    bipeds: Query<&BipedPawnComponent>,
    net_ids: Query<&NetworkID>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
    mut camera_fx: Query<(&mut CameraEffector, &GlobalTransform), With<Camera3d>>,
    mut predicted: Option<ResMut<common::PredictedCommands>>,
) {
    let Ok((mut ability, mut ability_state, ability_config)) = abilities.get_mut(input.ability)
    else {
        return;
    };
    let owner_biped = bipeds.get(input.owner).ok();
    if let Ok((mut camera_fx, camera_gt)) = camera_fx.single_mut() {
        let (_, rot, _) = camera_gt.to_scale_rotation_translation();
        let mut ctx = BipedAbilityCtx {
            ability: input.ability,
            pressed: input.pressed,
            held: input.held,
            alt_pressed: input.alt_pressed,
            alt_held: input.alt_held,
            origin: input.origin,
            aim_dir: rot * Vec3::NEG_Z,
            owner: input.owner,
            owner_biped,
            tick: input.tick,
            net_id: net_ids.get(input.ability).ok(),
            owner_net_id: net_ids.get(input.owner).ok(),
            sound: sound_queue.as_deref_mut(),
            camera: Some(&mut *camera_fx),
            quic: quic.as_deref_mut(),
            predicted: predicted.as_deref_mut(),
            state: &mut ability_state,
            config: ability_config.clone(),
        };
        ability.fixed_update(&mut world, &mut commands, &mut ctx);
    } else {
        let mut ctx = BipedAbilityCtx {
            ability: input.ability,
            pressed: input.pressed,
            held: input.held,
            alt_pressed: input.alt_pressed,
            alt_held: input.alt_held,
            origin: input.origin,
            aim_dir: Vec3::NEG_Z,
            owner: input.owner,
            owner_biped,
            tick: input.tick,
            net_id: net_ids.get(input.ability).ok(),
            owner_net_id: net_ids.get(input.owner).ok(),
            sound: sound_queue.as_deref_mut(),
            camera: None,
            quic: quic.as_deref_mut(),
            predicted: predicted.as_deref_mut(),
            state: &mut ability_state,
            config: ability_config.clone(),
        };
        ability.fixed_update(&mut world, &mut commands, &mut ctx);
    }
}

pub fn tick_biped_ability_state(
    mut abilities: Query<
        (&mut BipedAbilityState, &BipedAbilityConfig),
        With<BipedAbilityComponent>,
    >,
) {
    for (mut state, config) in &mut abilities {
        state.cooldown_ticks = state.cooldown_ticks.saturating_sub(1);
        state.active_ticks = state.active_ticks.saturating_sub(1);
        if config.meter_max > 0.0 {
            state.meter = (state.meter + config.meter_regen).min(config.meter_max);
        }
    }
}

pub fn start_cooldown(state: &mut BipedAbilityState, config: &BipedAbilityConfig) {
    state.cooldown_ticks = config.cooldown_ticks;
}

pub fn can_activate(state: &BipedAbilityState, config: &BipedAbilityConfig) -> bool {
    let _ = config;
    state.cooldown_ticks == 0
}

pub fn consume_charge(state: &mut BipedAbilityState, config: &BipedAbilityConfig) -> bool {
    if !can_activate(state, config) {
        return false;
    }
    state.active_ticks = config.active_ticks;
    start_cooldown(state, config);
    true
}

/// Drain `amount` from the meter. Returns `false` (and does not drain) if the meter is empty.
pub fn drain_meter(state: &mut BipedAbilityState, amount: f32) -> bool {
    if state.meter <= 0.0 {
        return false;
    }
    state.meter = (state.meter - amount).max(0.0);
    true
}

/// Returns `true` if the meter has at least `amount` remaining (or no meter system is in use).
pub fn has_meter(state: &BipedAbilityState, config: &BipedAbilityConfig, amount: f32) -> bool {
    config.meter_max == 0.0 || state.meter >= amount
}

/// Reads ability input and dispatches it to all ability entities owned by the possessed biped.
#[cfg(feature = "client")]
fn drive_biped_abilities(
    keyboard: Res<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>,
    mouse: Res<bevy::input::ButtonInput<bevy::input::mouse::MouseButton>>,
    egui_wants: Option<Res<bevy_egui::input::EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    ticker: Res<common::tick::Ticker>,
    possessed: Query<(Entity, &BipedPawnComponent), With<super::Possessed>>,
    pitch_pivots: Query<&GlobalTransform, With<super::PitchPivot>>,
    abilities: Query<(Entity, &BipedAbilityDriver, &AbilityOwner)>,
    mut commands: Commands,
) {
    let blocked = egui_wants.map_or(false, |e| e.wants_any_input());
    let Ok((pawn_entity, biped)) = possessed.single() else {
        return;
    };
    let Some(pitch_e) = biped.pitch_pivot else {
        return;
    };
    let Ok(gt) = pitch_pivots.get(pitch_e) else {
        return;
    };
    let (_, _, origin) = gt.to_scale_rotation_translation();

    let held = !blocked && bindings.pressed(common::InputAction::Ability, &keyboard, &mouse);
    let pressed =
        !blocked && bindings.just_pressed(common::InputAction::Ability, &keyboard, &mouse);

    for (ability_entity, driver, owner) in abilities.iter() {
        if owner.0 != pawn_entity {
            continue;
        }
        commands.run_system_with(
            driver.fixed_update,
            BipedAbilityInput {
                ability: ability_entity,
                pressed,
                held,
                alt_pressed: false,
                alt_held: false,
                origin,
                owner: pawn_entity,
                tick: ticker.tick,
            },
        );
    }
}
