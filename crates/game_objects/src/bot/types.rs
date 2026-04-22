use bevy::prelude::*;
use common::PawnInputKind;

use crate::Team;

pub struct BotContext {
    pub entity: Entity,
    pub team: Team,
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
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
            health: self.health,
            visible: Vec::new(),
        }
    }
}

pub struct BotOutput {
    pub input: PawnInputKind,
    pub fire: bool,
    pub reload: bool,
    pub aim_origin: Vec3,
    pub aim_dir: Vec3,
}

pub trait BotBrain: Send + Sync + 'static {
    fn think(&mut self, ctx: &BotContext) -> BotOutput;
}

#[derive(Component)]
pub struct BotController {
    pub team: Team,
    pub brain: Box<dyn BotBrain>,
    pub temp_id: u32,
}

impl BotController {
    pub fn new(team: Team, brain: impl BotBrain) -> Self {
        Self { team, brain: Box::new(brain), temp_id: 1 }
    }

    pub fn next_temp_id(&mut self) -> u32 {
        let id = self.temp_id;
        self.temp_id = self.temp_id.wrapping_add(1).max(1);
        id
    }
}
