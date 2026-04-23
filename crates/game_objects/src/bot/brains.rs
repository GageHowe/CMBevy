use bevy::prelude::*;
use common::{BipedInput, PawnInputKind};

use super::{BotBrain, BotContext, BotOutput};

pub struct HeuristicKillerBot;
impl Default for HeuristicKillerBot {
    fn default() -> Self {
        Self
    }
}

impl BotBrain for HeuristicKillerBot {
    fn think(&mut self, ctx: &BotContext) -> BotOutput {
        let Some(target) = ctx
            .visible
            .iter()
            .filter(|other| {
                other.entity != ctx.entity && other.team.0 != ctx.team.0 && other.health > 0.0
            })
            .min_by(|a, b| {
                ctx.pos.distance_squared(a.pos).total_cmp(&ctx.pos.distance_squared(b.pos))
            })
        else {
            return output(ctx, ctx.rot * Vec3::NEG_Z, 0.0, 0.0, false);
        };

        let up = ctx.rot * Vec3::Y;
        let noise = ((ctx.pos.x * 1.7 + ctx.pos.z * 0.9 + ctx.vel.length()).sin()).clamp(-1.0, 1.0);
        let lead = 0.25 + noise.abs() * 0.25;
        let aim = target.pos + target.vel * lead + up * (0.35 + noise * 0.2);
        let to_target = aim - ctx.pos;
        let distance = to_target.length();
        let forward = ((distance - 9.0) / 8.0).clamp(-0.4, 1.0);
        let right = if distance < 40.0 { noise * 0.65 } else { 0.0 };
        output(ctx, to_target.normalize_or_zero(), forward, right, distance < 90.0)
    }
}

fn output(ctx: &BotContext, aim_dir: Vec3, forward: f32, right: f32, fire: bool) -> BotOutput {
    let local_aim = ctx.rot.inverse() * aim_dir;
    let look_yaw = (-local_aim.x).atan2(-local_aim.z);
    let look_pitch = local_aim.y.clamp(-0.99, 0.99).asin();
    BotOutput {
        input: PawnInputKind::Biped(BipedInput {
            forward,
            right,
            look_yaw,
            look_pitch,
            ..default()
        }),
        fire,
        reload: false,
        aim_origin: ctx.pos + ctx.rot * Vec3::Y * 0.5,
        aim_dir,
    }
}
