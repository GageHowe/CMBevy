use crate::{NetworkID, PawnInputKind};
use bevy::prelude::*;
use std::collections::HashMap;

// I'm very proud of this.

#[derive(Clone)]
pub enum PredictedCommand {
    Input(PawnInputKind),
    Impulse {
        target: NetworkID,
        impulse: Vec3,
        point: Option<Vec3>,
    },
}

#[derive(Resource, Default)]
pub struct PredictedCommands {
    next_seq: u64,
    history: HashMap<u64, PredictedCommand>,
}
impl PredictedCommands {
    pub fn record(&mut self, command: PredictedCommand) -> u64 {
        let seq = self.next_seq.max(1);
        self.next_seq = seq + 1;
        self.history.insert(seq, command);
        self.history.retain(|&old_seq, _| old_seq + 128 >= seq);
        seq
    }
    pub fn record_input(&mut self, input: PawnInputKind) -> u64 {
        self.record(PredictedCommand::Input(input))
    }
    pub fn record_impulse(&mut self, target: NetworkID, impulse: Vec3) -> u64 {
        self.record_impulse_at(target, impulse, None)
    }
    pub fn record_impulse_at(
        &mut self,
        target: NetworkID,
        impulse: Vec3,
        point: Option<Vec3>,
    ) -> u64 {
        self.record(PredictedCommand::Impulse {
            target,
            impulse,
            point,
        })
    }
    pub fn get(&self, seq: u64) -> Option<&PredictedCommand> {
        self.history.get(&seq)
    }
    pub fn latest_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }
}
