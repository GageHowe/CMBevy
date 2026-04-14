use bevy::prelude::*;
use net::message::NetworkID;

#[derive(Component)]
pub struct WeaponSlots {
    pub slots: Vec<(Option<NetworkID>, Option<Entity>)>,
    pub active_index: usize,
    pub delete_on_out_of_ammo: bool,
    pub block_fire_until_release: bool,
}

impl Default for WeaponSlots {
    fn default() -> Self {
        Self::new(2)
    }
}

impl WeaponSlots {
    pub fn new(count: usize) -> Self {
        Self {
            slots: vec![(None, None); count.max(1)],
            active_index: 0,
            delete_on_out_of_ammo: false,
            block_fire_until_release: false,
        }
    }

    pub fn with_delete_on_out_of_ammo(mut self, delete_on_out_of_ammo: bool) -> Self {
        self.delete_on_out_of_ammo = delete_on_out_of_ammo;
        self
    }

    pub fn active(&self) -> &(Option<NetworkID>, Option<Entity>) {
        &self.slots[self.active_index]
    }

    pub fn is_full(&self) -> bool {
        self.slots.iter().all(|slot| slot.0.is_some())
    }

    pub fn active_primary(&self) -> bool {
        self.active_index == 0
    }

    pub fn set_active_primary(&mut self, active_primary: bool) {
        self.active_index = if active_primary {
            0
        } else {
            1.min(self.slots.len() - 1)
        };
    }

    pub fn contains_net_id(&self, id: &NetworkID) -> bool {
        self.slots.iter().any(|slot| slot.0.as_ref() == Some(id))
    }

    pub fn held_weapons(&self) -> impl Iterator<Item = (NetworkID, Entity)> + '_ {
        self.slots
            .iter()
            .filter_map(|(nid, ent)| nid.as_ref().zip(*ent))
            .map(|(nid, ent)| (nid.clone(), ent))
    }

    pub fn held_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.slots.iter().filter_map(|(_, ent)| *ent)
    }

    pub fn next_slot_index(&self) -> usize {
        (self.active_index + 1) % self.slots.len()
    }

    pub fn prev_slot_index(&self) -> usize {
        (self.active_index + self.slots.len() - 1) % self.slots.len()
    }

    pub fn next_slot(&mut self) -> bool {
        if self.slots.len() <= 1 {
            return false;
        }
        self.active_index = self.next_slot_index();
        true
    }

    pub fn prev_slot(&mut self) -> bool {
        if self.slots.len() <= 1 {
            return false;
        }
        self.active_index = self.prev_slot_index();
        true
    }

    pub fn next_weapon(&mut self) -> bool {
        let start = self.active_index;
        for _ in 0..self.slots.len().saturating_sub(1) {
            self.next_slot();
            if self.active().0.is_some() {
                return true;
            }
        }
        self.active_index = start;
        false
    }

    pub fn prev_weapon(&mut self) -> bool {
        let start = self.active_index;
        for _ in 0..self.slots.len().saturating_sub(1) {
            self.prev_slot();
            if self.active().0.is_some() {
                return true;
            }
        }
        self.active_index = start;
        false
    }

    pub fn remove_active(&mut self) -> Option<(NetworkID, Entity)> {
        let removed = Some((
            self.slots[self.active_index].0.take()?,
            self.slots[self.active_index].1.take()?,
        ));
        self.next_slot();
        removed
    }

    pub fn assign_pickup(
        &mut self,
        weapon_id: NetworkID,
        weapon_entity: Entity,
    ) -> Option<(bool, Option<Entity>)> {
        let start = self.active_index;
        for offset in 0..self.slots.len() {
            let idx = (start + offset) % self.slots.len();
            if self.slots[idx].0.is_some() {
                continue;
            }
            let prev = if idx == self.active_index {
                None
            } else {
                self.active().1
            };
            self.slots[idx] = (Some(weapon_id), Some(weapon_entity));
            self.active_index = idx;
            return Some((idx == 0, prev));
        }
        None
    }

    pub fn remove_by_net_id(&mut self, id: &NetworkID) {
        let removed_active = self.active().0.as_ref() == Some(id);
        for slot in &mut self.slots {
            if slot.0.as_ref() == Some(id) {
                *slot = (None, None);
            }
        }
        if removed_active {
            self.next_slot();
        }
    }

    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = (None, None);
        }
        self.active_index = 0;
        self.block_fire_until_release = false;
    }
}
