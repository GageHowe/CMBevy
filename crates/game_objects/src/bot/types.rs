use bevy::prelude::*;
use common::PawnInputKind;
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
    pub health: f32,
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

/// Decision output emitted by a bot brain for the current think step.
pub struct BotOutput {
    pub input: PawnInputKind,
    pub fire: bool,
    pub reload: bool,
    pub aim_origin: Vec3,
    pub aim_dir: Vec3,
}

/// Behaviour interface implemented by server-side bot brains.
pub trait BotBrain: Send + Sync + 'static {
    fn think(&mut self, ctx: &BotContext) -> BotOutput;
}

#[derive(Component)]
/// Runtime bot controller component attached to a pawn entity.
pub struct BotController {
    pub team: Team,
    pub brain: Box<dyn BotBrain>,
    /// Temporary projectile id counter for locally generated authoritative bot shots.
    /// TODO: bots will either be in singleplayer or server-authoritative, so this isn't needed
    pub temp_id: u32,
}

impl BotController {
    pub fn new(team: Team, brain: impl BotBrain) -> Self {
        Self {
            team,
            brain: Box::new(brain),
            temp_id: 1,
        }
    }

    pub fn next_temp_id(&mut self) -> u32 {
        let id = self.temp_id;
        self.temp_id = self.temp_id.wrapping_add(1).max(1);
        id
    }
}

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
