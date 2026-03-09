use bevy::prelude::*;

/// Marks an entity as interactable via the F key.
/// Any networked entity (weapon, vehicle, etc.) can carry this component.
/// The client finds the nearest one in range and sends `MsgType::Interact(net_id)` to the server.
#[derive(Component)]
pub struct Interactable {
    pub range: f32,
}
