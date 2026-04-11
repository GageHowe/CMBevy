use crate::pawn::CameraEffector;
use crate::sound::SoundQueue;
#[cfg(feature = "client")]
use bevy::ecs::system::{In, SystemId};
use bevy::prelude::*;
use net::message::NetworkID;
use physics::physics_world::PhysicsWorld;

pub mod hail_mary;
pub mod helpers;
pub mod pistol;
pub mod rifle;
pub mod rpg;

/// Shared weapon plugin.
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            rifle::RiflePlugin,
            pistol::PistolPlugin,
            hail_mary::HailMaryPlugin,
            rpg::RpgPlugin,
            crate::projectile::ProjectilePlugin,
        ))
        .add_systems(FixedUpdate, tick_weapon_state);
    }
}

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

#[derive(Component, Clone, Reflect)]
pub struct WeaponConfig {
    pub magazine_size: u16,
    pub reload_ticks: u16,
    pub fire_cooldown_ticks: u16,
    pub projectile_kind: net::message::GameObjectKind,
}

#[derive(Component, Clone, Copy, Reflect, Default)]
pub struct WeaponState {
    pub ammo_in_mag: u16,
    pub reserve_ammo: u16,
    pub reload_ticks: u16,
    pub cooldown_ticks: u16,
}

impl WeaponState {
    pub fn snapshot(self) -> common::WeaponStateSnapshot {
        common::WeaponStateSnapshot {
            ammo_in_mag: self.ammo_in_mag,
            reserve_ammo: self.reserve_ammo,
            reload_ticks: self.reload_ticks,
            cooldown_ticks: self.cooldown_ticks,
        }
    }

    pub fn apply_snapshot(&mut self, snapshot: common::WeaponStateSnapshot) {
        self.ammo_in_mag = snapshot.ammo_in_mag;
        self.reserve_ammo = snapshot.reserve_ammo;
        self.reload_ticks = snapshot.reload_ticks;
        self.cooldown_ticks = snapshot.cooldown_ticks;
    }
}

/// Type-erased fire hook for the weapon entity. Biped code only forwards input here.
#[cfg(feature = "client")]
#[derive(Component, Clone, Copy)]
pub struct WeaponDriver {
    pub fixed_update: SystemId<In<WeaponFireInput>>,
}

#[cfg(feature = "client")]
#[derive(Clone, Copy)]
pub struct WeaponFireInput {
    pub weapon: Entity,
    pub want_fire: bool,
    pub want_alt_fire: bool,
    pub reload_pressed: bool,
    pub origin: Vec3,
    pub shooter: Entity,
    pub tick: u64,
}

/// UI reads this from the active controllable object so reticle selection stays gameplay-owned.
#[derive(Component, Clone, Copy)]
pub struct AimReticle(pub &'static str, pub Option<f32>);

/// All context a weapon's fixed_update may need: input buttons and output channels.
/// Fields are optional so weapons compile and behave correctly on the server (no sound/camera).
pub struct FireCtx<'a> {
    pub weapon: Entity,
    pub want_fire: bool,
    pub want_alt_fire: bool,
    pub reload_pressed: bool,
    pub origin: Vec3,
    pub aim_dir: Vec3,
    pub shooter: Option<Entity>,
    pub tick: u64,
    /// NetworkID of the weapon entity — used to tell the server what fired.
    pub net_id: Option<&'a NetworkID>,
    /// NetworkID of the pawn carrying the weapon — included in fire packets so other clients can find the ghost.
    pub shooter_net_id: Option<&'a NetworkID>,
    /// Push to play a one-shot sound this frame.
    pub sound: Option<&'a mut SoundQueue>,
    /// Local player camera; None on server or before possession.
    pub camera: Option<&'a mut CameraEffector>,
    /// QUIC manager for sending Fire messages; None in singleplayer.
    pub quic: Option<&'a mut net::quic::QuicManager>,
    /// Projectile id counter for client-side prediction; None in singleplayer / on server.
    pub id_counter: Option<&'a mut u32>,
    /// Local predicted command history so weapons can replay non-input impulses during reconciliation.
    pub predicted: Option<&'a mut common::PredictedCommands>,
    pub weapon_state: &'a mut WeaponState,
    pub weapon_config: WeaponConfig,
}

/// Per-weapon-type firing logic. Implement on each weapon component.
/// Weapons own their complete fire behavior: cooldowns, projectiles, sounds, camera kick, networking.
pub trait Weapon: Component<Mutability = bevy::ecs::component::Mutable> + Default {
    const MODEL_PATH: &'static str;
    const COLLIDER_PATH: &'static str;
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair013.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = None;
    const ZOOM_MULTIPLIER: f32 = 1.0;
    const MAGAZINE_SIZE: u16;
    const RESERVE_AMMO: u16;
    const RELOAD_TICKS: u16;
    const FIRE_COOLDOWN_TICKS: u16;
    const PROJECTILE_KIND: net::message::GameObjectKind;
    /// Called every FixedPreUpdate tick when this weapon is the active slot.
    /// The weapon reads input from ctx, spawns projectiles/effects, and calls ctx helpers as needed.
    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    );
}

#[cfg(feature = "client")]
pub fn weapon_bundle<W: Weapon + 'static>(weapon: W, world: &mut World) -> impl Bundle {
    (
        weapon,
        WeaponConfig::new::<W>(),
        WeaponState::new::<W>(),
        WeaponDriver {
            fixed_update: world.register_system_cached(fire_weapon::<W>),
        },
    )
}

#[cfg(not(feature = "client"))]
pub fn weapon_bundle<W: Weapon>(weapon: W, _world: &mut World) -> impl Bundle {
    (weapon, WeaponConfig::new::<W>(), WeaponState::new::<W>())
}

#[cfg(feature = "client")]
pub fn fire_weapon<W: Weapon>(
    In(input): In<WeaponFireInput>,
    mut weapons: Query<(&mut W, &mut WeaponState, &WeaponConfig)>,
    net_ids: Query<&NetworkID>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
    mut camera_fx: Query<(&mut CameraEffector, &GlobalTransform), With<Camera3d>>,
    mut id_counter: Option<ResMut<crate::projectile::ProjectileIdCounter>>,
    mut predicted: Option<ResMut<common::PredictedCommands>>,
) {
    let Ok((mut weapon, mut weapon_state, weapon_config)) = weapons.get_mut(input.weapon) else {
        return;
    };
    // use camera's GlobalTransform for aim so kick offsets affect projectile direction
    let Ok((mut cam_fx, cam_gt)) = camera_fx.single_mut() else {
        return;
    };
    let (_, cam_rot, _) = cam_gt.to_scale_rotation_translation();
    let mut ctx = FireCtx {
        weapon: input.weapon,
        want_fire: input.want_fire,
        want_alt_fire: input.want_alt_fire,
        reload_pressed: input.reload_pressed,
        origin: input.origin,
        aim_dir: cam_rot * Vec3::NEG_Z,
        shooter: Some(input.shooter),
        tick: input.tick,
        net_id: net_ids.get(input.weapon).ok(),
        shooter_net_id: net_ids.get(input.shooter).ok(),
        sound: sound_queue.as_deref_mut(),
        camera: Some(&mut *cam_fx),
        quic: quic.as_deref_mut(),
        id_counter: id_counter.as_mut().map(|c| &mut c.count),
        predicted: predicted.as_deref_mut(),
        weapon_state: &mut weapon_state,
        weapon_config: weapon_config.clone(),
    };
    weapon.fixed_update(&mut world, &mut commands, &mut ctx);
}

pub fn default_crosshair_path() -> &'static str {
    "textures/crosshairs/crosshair001.png"
}

pub fn apply_zoom<W: Weapon>(ctx: &mut FireCtx) -> f32 {
    let Some(cam) = ctx.camera.as_mut() else {
        return 0.0;
    };
    let zoom_multiplier = if ctx.want_alt_fire {
        W::ZOOM_MULTIPLIER
    } else {
        1.0
    }
    .max(1.0);
    cam.zoom_multiplier = zoom_multiplier;
    if zoom_multiplier <= 1.0 {
        return 0.0;
    }
    ((cam.current_zoom_factor() - 1.0) / (zoom_multiplier - 1.0)).clamp(0.0, 1.0)
}

impl WeaponConfig {
    pub fn new<W: Weapon>() -> Self {
        Self {
            magazine_size: W::MAGAZINE_SIZE,
            reload_ticks: W::RELOAD_TICKS,
            fire_cooldown_ticks: W::FIRE_COOLDOWN_TICKS,
            projectile_kind: W::PROJECTILE_KIND,
        }
    }
}

impl WeaponState {
    pub fn new<W: Weapon>() -> Self {
        Self {
            ammo_in_mag: W::MAGAZINE_SIZE,
            reserve_ammo: W::RESERVE_AMMO,
            reload_ticks: 0,
            cooldown_ticks: 0,
        }
    }
}

pub fn tick_weapon_state(
    mut weapons: Query<(&mut WeaponState, &WeaponConfig), With<WeaponComponent>>,
) {
    for (mut state, config) in &mut weapons {
        state.cooldown_ticks = state.cooldown_ticks.saturating_sub(1);
        if state.reload_ticks == 0 {
            continue;
        }
        state.reload_ticks -= 1;
        if state.reload_ticks > 0 {
            continue;
        }
        let need = config.magazine_size.saturating_sub(state.ammo_in_mag);
        if need == 0 || state.reserve_ammo == 0 {
            continue;
        }
        let refill = need.min(state.reserve_ammo);
        state.ammo_in_mag += refill;
        state.reserve_ammo -= refill;
    }
}

pub fn start_reload(state: &mut WeaponState, config: &WeaponConfig) -> bool {
    if state.reload_ticks > 0
        || state.ammo_in_mag >= config.magazine_size
        || state.reserve_ammo == 0
    {
        return false;
    }
    state.reload_ticks = config.reload_ticks;
    true
}

pub fn cancel_reload(state: &mut WeaponState) {
    state.reload_ticks = 0;
}

pub fn can_fire(state: &WeaponState) -> bool {
    state.reload_ticks == 0 && state.cooldown_ticks == 0 && state.ammo_in_mag > 0
}

pub fn is_depleted(state: &WeaponState) -> bool {
    state.ammo_in_mag == 0 && state.reserve_ammo == 0
}

pub fn consume_round(state: &mut WeaponState, config: &WeaponConfig) -> bool {
    if !can_fire(state) {
        if state.reload_ticks == 0 && state.cooldown_ticks == 0 && state.ammo_in_mag == 0 {
            start_reload(state, config);
        }
        return false;
    }
    state.ammo_in_mag -= 1;
    state.cooldown_ticks = config.fire_cooldown_ticks;
    // Empty mags should immediately enter reload so client prediction and server authority
    // stay on the same state path after the last shot.
    if state.ammo_in_mag == 0 {
        start_reload(state, config);
    }
    true
}

pub fn is_weapon_kind(kind: &common::GameObjectKind) -> bool {
    matches!(
        kind,
        common::GameObjectKind::Pistol
            | common::GameObjectKind::Rifle
            | common::GameObjectKind::HailMary
            | common::GameObjectKind::Rpg
    )
}
