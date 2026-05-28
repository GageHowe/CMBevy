use bevy::prelude::*;

/// Presentation hint attached to a controllable object or weapon that wants a custom crosshair.
#[derive(Component, Clone, Copy)]
pub struct AimReticle(pub &'static str, pub Option<f32>);

/// Local-space aim origin entity used for reticle prediction and other free-look presentation.
#[derive(Component, Clone, Copy)]
pub struct AimOrigin(pub Entity);

/// Fallback crosshair used when the controlled pawn and active weapon provide no override.
pub fn default_crosshair_path() -> &'static str {
    "textures/crosshairs/crosshair001.png"
}
