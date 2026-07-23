use std::collections::{BTreeMap, VecDeque};

use bevy::prelude::*;

use crate::{NetworkID, PawnInput};

const HISTORY_LEN: usize = 128;

#[derive(Resource, Default)]
pub struct LocalControl {
    pending: VecDeque<PawnInput>,
    history: VecDeque<(u64, PawnInput)>,
    next_seq: u64,
    sent_seq: u64,
}

impl LocalControl {
    pub fn record_input(&mut self, input: PawnInput) -> u64 {
        let seq = self.next_seq.max(1);
        self.next_seq = seq + 1;
        if self.history.len() == HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back((seq, input));
        seq
    }

    pub fn push(&mut self, input: PawnInput) {
        if self.pending.len() == HISTORY_LEN {
            self.pending.pop_front();
        }
        self.pending.push_back(input.clone());
        self.record_input(input);
    }

    pub fn consume(&mut self) -> Option<PawnInput> {
        self.pending.pop_front()
    }

    pub fn take_newest_to_send(&mut self) -> Option<(u64, PawnInput)> {
        let (seq, input) = self.history.back()?;
        if *seq <= self.sent_seq {
            return None;
        }
        self.sent_seq = *seq;
        Some((*seq, input.clone()))
    }

    pub fn newest_mut(&mut self) -> Option<&mut PawnInput> {
        self.history.back_mut().map(|(_, input)| input)
    }

    pub fn get(&self, seq: u64) -> Option<&PawnInput> {
        let (first_seq, _) = self.history.front()?;
        self.history
            .get(seq.checked_sub(*first_seq)? as usize)
            .and_then(|(stored_seq, input)| (*stored_seq == seq).then_some(input))
    }

    pub fn latest_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }
}

#[derive(Clone)]
pub struct PredictedImpulse {
    pub target: NetworkID,
    pub impulse: Vec3,
    pub point: Option<Vec3>,
}

#[derive(Resource, Default)]
pub struct PredictedImpulses(pub BTreeMap<u64, Vec<PredictedImpulse>>);

impl PredictedImpulses {
    pub fn record(&mut self, seq: u64, target: NetworkID, impulse: Vec3, point: Option<Vec3>) {
        let impulses = self.0.entry(seq).or_default();
        if let Some(existing) = impulses
            .iter_mut()
            .find(|existing| existing.target == target && existing.point == point)
        {
            existing.impulse += impulse;
        } else {
            impulses.push(PredictedImpulse {
                target,
                impulse,
                point,
            });
        }
        while self.0.len() > HISTORY_LEN {
            self.0.pop_first();
        }
    }

    pub fn get(&self, seq: u64) -> Option<&[PredictedImpulse]> {
        self.0.get(&seq).map(Vec::as_slice)
    }
}
