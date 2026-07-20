use bevy::prelude::*;
use common::PawnInput;
use physics::physics_world::{PhysicsWorld, rb_pos, rb_rot, rb_vel};

use crate::{Team, health::Health, pawn::WeaponSlots, reticle::AimReticle};

/// Snapshot of one actor used as input to a bot brain for a single think step.
pub struct BotContext {
    pub entity: Entity,
    pub team: Team,
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub projectile_speed: Option<f32>,
    pub health: i32,
    pub visible: Vec<BotContext>,
}

impl Clone for BotContext {
    fn clone(&self) -> Self {
        Self {
            entity: self.entity,
            team: self.team,
            pos: self.pos,
            rot: self.rot,
            vel: self.vel,
            projectile_speed: self.projectile_speed,
            health: self.health,
            visible: Vec::new(),
        }
    }
}

/// Decision output emitted by a bot brain for the current think step. TODO: investigate changing to dedicated pawn input structs
pub struct BotOutput {
    pub input: PawnInput,
    pub fire: bool,
    pub reload: bool,
    pub aim_origin: Vec3,
    pub aim_dir: Vec3,
}

/// Behaviour interface implemented by server-side bot brains.
pub trait BotBehavior: Send + Sync + 'static {
    /// outputs a predefined input struct; perhaps should be defined in terms of pawn input structs. e.g. biped can output a BipedInput struct
    fn think(&mut self, ctx: &BotContext) -> BotOutput;
}

/// attach this to an entity to give it bot behavior! :)
#[derive(Component)]
pub struct BotController {
    pub team: Team,
    pub brain: Box<dyn BotBehavior>,
}
impl BotController {
    pub fn new(team: Team, brain: impl BotBehavior) -> Self {
        Self {
            team,
            brain: Box::new(brain),
        }
    }
}

/// TODO: make this function more efficient:
/// - only gather nearby objects
/// - possibly reuse contexts for bots who are close together
pub fn collect_contexts(
    actors: &Query<(Entity, &Team, &Health)>,
    slots: &Query<&WeaponSlots>,
    reticles: &Query<&AimReticle>,
    world: &PhysicsWorld,
) -> Vec<BotContext> {
    actors
        .iter()
        .filter_map(|(entity, team, health)| {
            let body = world
                .entity_to_handle
                .get(&entity)
                .and_then(|handle| world.rigid_body_set.get(*handle))?;
            Some(BotContext {
                entity,
                team: *team,
                pos: rb_pos(body),
                rot: rb_rot(body),
                vel: rb_vel(body),
                projectile_speed: projectile_speed(entity, slots, reticles),
                health: health.current,
                visible: Vec::new(),
            })
        })
        .collect()
}

fn projectile_speed(
    entity: Entity,
    slots: &Query<&WeaponSlots>,
    reticles: &Query<&AimReticle>,
) -> Option<f32> {
    reticles
        .get(entity)
        .ok()
        .and_then(|reticle| reticle.1)
        .or_else(|| {
            let weapon_entity = slots.get(entity).ok()?.active_weapon()?.1;
            reticles
                .get(weapon_entity)
                .ok()
                .and_then(|reticle| reticle.1)
        })
}
