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
        let target = ctx
            .visible
            .iter()
            .filter(|other| {
                other.entity != ctx.entity && other.team.0 != ctx.team.0 && other.health > 0.0
            })
            .min_by(|a, b| {
                ctx.pos.distance_squared(a.pos).total_cmp(&ctx.pos.distance_squared(b.pos))
            });
        let Some(target) = target else {
            let forward = ctx.rot * Vec3::NEG_Z;
            return biped_output(ctx, forward, forward, 0.0, 0.0, false);
        };

        let up = ctx.rot * Vec3::Y;
        let predicted_target = target.pos + target.vel * 0.35;
        let to_target = predicted_target - ctx.pos;
        let distance = to_target.length();
        let move_dir = (to_target - up * to_target.dot(up)).normalize_or_zero();
        let aim_dir = (predicted_target + up * 0.5 - (ctx.pos + up * 0.5)).normalize_or_zero();
        let speed = if distance > 14.0 { 1.0 } else if distance < 7.0 { -0.35 } else { 0.0 };
        let strafe = if distance < 35.0 { (ctx.pos.x * 0.7).sin() * 0.55 } else { 0.0 };
        biped_output(ctx, move_dir, aim_dir, speed, strafe, distance < 80.0)
    }
}

fn biped_output(
    ctx: &BotContext,
    move_dir: Vec3,
    aim_dir: Vec3,
    forward: f32,
    right: f32,
    fire: bool,
) -> BotOutput {
    let local_move = ctx.rot.inverse() * move_dir.normalize_or_zero();
    let local_aim = ctx.rot.inverse() * aim_dir.normalize_or_zero();
    let look_yaw = (-local_move.x).atan2(-local_move.z);
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
