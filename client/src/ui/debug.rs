use bevy::{prelude::*, ui::Val};
use bevy_dev_tools::diagnostics_overlay::DiagnosticsOverlay;

use crate::settings::Settings;

/// top left Bevy diagnostics overlay controlled by Settings::debug_panel.
pub fn sync_diagnostics_overlay(
    mut commands: Commands,
    settings: Res<Settings>,
    overlays: Query<Entity, With<DiagnosticsOverlay>>,
    mut overlay_nodes: Query<(&DiagnosticsOverlay, &mut Node)>,
) {
    if settings.debug_panel && overlays.is_empty() {
        commands.spawn(DiagnosticsOverlay::fps());
        commands.spawn(DiagnosticsOverlay::mesh_and_standard_material());
    } else if !settings.debug_panel {
        for entity in &overlays {
            commands.entity(entity).despawn();
        }
    }
    for (overlay, mut node) in &mut overlay_nodes {
        node.left = Val::Px(32.0);
        node.top = Val::Px(if overlay.title.as_ref() == "Fps" {
            32.0
        } else {
            128.0
        });
    }
}
