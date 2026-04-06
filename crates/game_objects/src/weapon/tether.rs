use super::{FireCtx, Weapon, helpers};
use crate::projectile::tether;
use crate::{GameObject, GameObjectKind};
use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::{ColliderBuilder, ImpulseJointHandle};

const HULL_PATH: &str = "collision/placeholder_ar.obj";
const COOLDOWN_TICKS: u32 = 12;

pub struct TetherGunPlugin;
impl Plugin for TetherGunPlugin {
    fn build(&self, _app: &mut App) {
        #[cfg(feature = "client")]
        _app.add_systems(Update, draw_tether_lines);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Reflect)]
pub enum TetherSide {
    Left,
    Right,
}

impl TetherSide {
    pub fn bit(self) -> u32 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }

    pub fn from_temp_id(temp_id: u32) -> Self {
        if temp_id & 1 == 1 {
            Self::Right
        } else {
            Self::Left
        }
    }
}

#[derive(Clone, Copy, Reflect)]
pub struct TetherEndpoint {
    pub entity: Entity,
    pub local_anchor: Vec3,
}

#[derive(Component, Reflect)]
pub struct TetherGunComponent {
    pub cooldown: u32,
    pub left: Option<TetherEndpoint>,
    pub right: Option<TetherEndpoint>,
    #[reflect(ignore)]
    pub joint: Option<ImpulseJointHandle>,
}

impl Default for TetherGunComponent {
    fn default() -> Self {
        Self {
            cooldown: 0,
            left: None,
            right: None,
            joint: None,
        }
    }
}

impl TetherGunComponent {
    fn endpoint_mut(&mut self, side: TetherSide) -> &mut Option<TetherEndpoint> {
        match side {
            TetherSide::Left => &mut self.left,
            TetherSide::Right => &mut self.right,
        }
    }

    fn other_endpoint_mut(&mut self, side: TetherSide) -> &mut Option<TetherEndpoint> {
        match side {
            TetherSide::Left => &mut self.right,
            TetherSide::Right => &mut self.left,
        }
    }

    fn clear(&mut self, world: &mut PhysicsWorld) {
        if let Some(joint) = self.joint.take() {
            world.remove_impulse_joint(joint);
        }
        self.left = None;
        self.right = None;
    }
}

impl Weapon for TetherGunComponent {
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair013.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = None;

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        self.cooldown = self.cooldown.saturating_sub(1);
        let side = if ctx.want_fire {
            Some(TetherSide::Left)
        } else if ctx.want_alt_fire {
            Some(TetherSide::Right)
        } else {
            None
        };
        let Some(side) = side else {
            return;
        };
        if self.cooldown > 0 {
            return;
        }
        self.cooldown = COOLDOWN_TICKS;
        if self.left.is_some() && self.right.is_some() {
            self.clear(world);
        }

        let velocity = helpers::projectile_velocity(world, ctx.shooter, ctx.aim_dir, tether::SPEED);
        let temp_id = (helpers::next_temp_id(ctx.id_counter.as_deref_mut()) << 1) | side.bit();
        tether::spawn(
            ctx.origin,
            velocity,
            commands,
            world,
            ctx.shooter,
            Some(ctx.weapon),
            side,
            temp_id,
        );
        #[cfg(feature = "client")]
        helpers::send_fire_request(
            ctx.quic.as_deref_mut(),
            ctx.net_id,
            GameObjectKind::TetherHookProjectile,
            temp_id,
            ctx.origin,
            ctx.aim_dir,
        );
    }
}

impl GameObject for TetherGunComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::TetherGun,
            <Self as Weapon>::CROSSHAIR_PATH,
            <Self as Weapon>::PREDICTION_PROJECTILE_SPEED,
            TetherGunComponent::default(),
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd,
            HULL_PATH,
            ColliderBuilder::cuboid(0.18, 0.06, 0.32),
            world,
        );
    }
}

pub fn attach_endpoint(
    weapon: &mut TetherGunComponent,
    side: TetherSide,
    shooter: Option<Entity>,
    target: Entity,
    hit_point: Vec3,
    player_anchor: Vec3,
    world: &mut PhysicsWorld,
    create_joint: bool,
) {
    if weapon.left.is_some() && weapon.right.is_some() {
        weapon.clear(world);
    } else if let Some(joint) = weapon.joint.take() {
        world.remove_impulse_joint(joint);
    }
    let Some(hit_endpoint) = endpoint_from_world(world, target, hit_point) else {
        return;
    };
    *weapon.endpoint_mut(side) = Some(hit_endpoint);
    if weapon.other_endpoint_mut(side).is_none()
        && let Some(shooter) = shooter
        && let Some(player_endpoint) = endpoint_from_world(world, shooter, player_anchor)
    {
        *weapon.other_endpoint_mut(side) = Some(player_endpoint);
    }
    if create_joint {
        rebuild_joint(weapon, world);
    }
}

fn endpoint_from_world(
    world: &PhysicsWorld,
    entity: Entity,
    world_anchor: Vec3,
) -> Option<TetherEndpoint> {
    let rb = world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))?;
    Some(TetherEndpoint {
        entity,
        local_anchor: rb_rot(rb).inverse() * (world_anchor - rb_pos(rb)),
    })
}

fn endpoint_world(world: &PhysicsWorld, endpoint: TetherEndpoint) -> Option<Vec3> {
    let rb = world
        .entity_to_handle
        .get(&endpoint.entity)
        .and_then(|&h| world.rigid_body_set.get(h))?;
    Some(rb_pos(rb) + rb_rot(rb) * endpoint.local_anchor)
}

fn rebuild_joint(weapon: &mut TetherGunComponent, world: &mut PhysicsWorld) {
    let (Some(left), Some(right)) = (weapon.left, weapon.right) else {
        return;
    };
    if left.entity == right.entity {
        return;
    }
    let (Some(left_world), Some(right_world)) =
        (endpoint_world(world, left), endpoint_world(world, right))
    else {
        return;
    };
    let max_dist = left_world.distance(right_world).max(0.25);
    weapon.joint = world.insert_rope_joint(
        left.entity,
        right.entity,
        left.local_anchor,
        right.local_anchor,
        max_dist,
        false,
    );
}

pub fn sync_endpoints(
    weapon: &mut TetherGunComponent,
    left: Option<TetherEndpoint>,
    right: Option<TetherEndpoint>,
    world: &mut PhysicsWorld,
) {
    if let Some(joint) = weapon.joint.take() {
        world.remove_impulse_joint(joint);
    }
    weapon.left = left;
    weapon.right = right;
    rebuild_joint(weapon, world);
}

#[cfg(feature = "client")]
fn draw_tether_lines(
    world: Res<PhysicsWorld>,
    weapons: Query<&TetherGunComponent>,
    mut gizmos: Gizmos,
) {
    for weapon in &weapons {
        let (Some(left), Some(right)) = (weapon.left, weapon.right) else {
            continue;
        };
        let (Some(left), Some(right)) =
            (endpoint_world(&world, left), endpoint_world(&world, right))
        else {
            continue;
        };
        gizmos.line(left, right, Color::srgb(0.7, 0.9, 1.0));
    }
}
