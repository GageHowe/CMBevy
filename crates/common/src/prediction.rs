use std::collections::HashMap;

use bevy::prelude::*;

use crate::{NetworkID, PawnInputKind};

#[derive(Clone)]
pub struct PredictedImpulse {
    pub target: NetworkID,
    pub impulse: Vec3,
    pub point: Option<Vec3>,
}

#[derive(Clone)]
pub struct PredictedTick {
    pub input: PawnInputKind,
    pub impulses: Vec<PredictedImpulse>,
}

#[derive(Resource, Default)]
pub struct PredictedCommands {
    next_seq: u64,
    history: HashMap<u64, PredictedTick>,
}

impl PredictedCommands {
    pub fn record_input(&mut self, input: PawnInputKind) -> u64 {
        let seq = self.next_seq.max(1);
        self.next_seq = seq + 1;
        self.history.insert(seq, PredictedTick { input, impulses: Vec::new() });
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
        tick.impulses.push(PredictedImpulse { target, impulse, point });
    }

    pub fn get(&self, seq: u64) -> Option<&PredictedTick> {
        self.history.get(&seq)
    }

    pub fn latest_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }
}
