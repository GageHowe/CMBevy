use std::collections::HashMap;

use bevy::prelude::*;

use crate::{NetworkID, PawnInput};

/// A locally predicted impulse that must be replayed during rollback.
/// Discrete things like jump, self-knockback, etc.
#[derive(Clone)]
pub struct PredictedImpulse {
    /// networked body that received the impulse
    pub target: NetworkID,
    /// World-space impulse applied to the target.
    pub impulse: Vec3,
    /// Optional world-space contact point for point impulses.
    pub point: Option<Vec3>,
}

/// All locally predicted commands recorded for one input sequence.
#[derive(Clone)]
pub struct PredictedTick {
    /// Input sent to the server for this tick.
    pub input: PawnInput,
    /// Extra side effects predicted locally on top of the raw input.
    pub impulses: Vec<PredictedImpulse>,
}

/// Ring-buffer style history used by client reconciliation.
#[derive(Resource, Default)]
pub struct PredictedCommands {
    next_seq: u64,
    history: HashMap<u64, PredictedTick>,
}

impl PredictedCommands {
    pub fn record_input(&mut self, input: PawnInput) -> u64 {
        let seq = self.next_seq.max(1);
        self.next_seq = seq + 1;
        self.history.insert(
            seq,
            PredictedTick {
                input,
                impulses: Vec::new(),
            },
        );
        self.history.retain(|&old_seq, _| old_seq + 128 >= seq);
        seq
    }

    pub fn record_impulse(&mut self, target: NetworkID, impulse: Vec3) {
        self.record_impulse_at(target, impulse, None);
    }

    pub fn record_impulse_at(&mut self, target: NetworkID, impulse: Vec3, point: Option<Vec3>) {
        let seq = self.latest_seq();
        if seq == 0 {
            return;
        }
        let Some(tick) = self.history.get_mut(&seq) else {
            return;
        };
        if let Some(existing) = tick
            .impulses
            .iter_mut()
            .find(|existing| existing.target == target && existing.point == point)
        {
            existing.impulse += impulse;
            return;
        }
        tick.impulses.push(PredictedImpulse {
            target,
            impulse,
            point,
        });
    }

    pub fn get(&self, seq: u64) -> Option<&PredictedTick> {
        self.history.get(&seq)
    }

    pub fn latest_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }
}
