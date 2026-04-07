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
        ));
    }
}

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

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
}

/// Per-weapon-type firing logic. Implement on each weapon component.
/// Weapons own their complete fire behavior: cooldowns, projectiles, sounds, camera kick, networking.
pub trait Weapon: Component<Mutability = bevy::ecs::component::Mutable> + Default {
    const MODEL_PATH: &'static str;
    const COLLIDER_PATH: &'static str;
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair013.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = None;
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
        WeaponDriver {
            fixed_update: world.register_system_cached(fire_weapon::<W>),
        },
    )
}

#[cfg(not(feature = "client"))]
pub fn weapon_bundle<W: Weapon>(weapon: W, _world: &mut World) -> impl Bundle {
    weapon
}

#[cfg(feature = "client")]
pub fn fire_weapon<W: Weapon>(
    In(input): In<WeaponFireInput>,
    mut weapons: Query<&mut W>,
    net_ids: Query<&NetworkID>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
    mut camera_fx: Query<(&mut CameraEffector, &GlobalTransform), With<Camera3d>>,
    mut id_counter: Option<ResMut<crate::projectile::ProjectileIdCounter>>,
    mut predicted: Option<ResMut<common::PredictedCommands>>,
) {
    let Ok(mut weapon) = weapons.get_mut(input.weapon) else {
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
    };
    weapon.fixed_update(&mut world, &mut commands, &mut ctx);
}

pub fn default_crosshair_path() -> &'static str {
    "textures/crosshairs/crosshair001.png"
}
