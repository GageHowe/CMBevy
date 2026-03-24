use serde_json;
use std::collections::HashMap;

use common::{GameObjectKind, NetworkID};

#[test]
fn game_object_kind_default_is_biped() {
    // ensure default variant is Biped
    assert_eq!(GameObjectKind::default(), GameObjectKind::Biped);
}

#[test]
fn network_id_resource_next_increments() {
    let mut res = common::NetworkIDResource::default();
    assert_eq!(res.next(), 1);
    assert_eq!(res.next(), 2);
}

// Note: Bodystate/SimulationState involve Bevy types (Vec3/Quat) which require Bevy's
// ECS types in test context. We keep lightweight tests here, focusing on simple,
// verifiable, Bevy-independent aspects of the common crate.
