use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::pbr::MeshMaterial3d;
use serde::{Deserialize, Serialize};

pub struct BlackHolePlugin;
impl Plugin for BlackHolePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<BlackHole>();

        #[cfg(feature = "client")]
        app.add_systems(
            Update,
            (
                spawn_black_holes,
                cleanup_black_holes,
                sync_black_hole_visuals,
            ),
        );
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct BlackHole {
    pub event_horizon_radius: f32,
    pub photon_ring_width: f32,
    pub lensing_radius: f32,
    pub disk_inner_radius: f32,
    pub disk_outer_radius: f32,
    pub disk_half_thickness: f32,
    pub disk_color: Color,
    pub ring_color: Color,
    pub disk_brightness: f32,
    pub ring_brightness: f32,
    pub distortion_strength: f32,
}

impl Default for BlackHole {
    fn default() -> Self {
        Self {
            event_horizon_radius: 20.0,
            photon_ring_width: 3.5,
            lensing_radius: 90.0,
            disk_inner_radius: 26.0,
            disk_outer_radius: 62.0,
            disk_half_thickness: 4.0,
            disk_color: Color::srgb(1.0, 0.72, 0.3),
            ring_color: Color::srgb(1.0, 0.96, 0.9),
            disk_brightness: 3.5,
            ring_brightness: 2.8,
            distortion_strength: 1.35,
        }
    }
}

#[cfg(feature = "client")]
pub fn draw_black_hole_radii(
    black_holes: Query<(&BlackHole, &GlobalTransform)>,
    mut gizmos: Gizmos,
) {
    for (black_hole, gt) in &black_holes {
        let pos = gt.translation();
        crate::debug_draw::draw_radius_spheres(
            &mut gizmos,
            pos,
            black_hole.event_horizon_radius.max(0.0),
            black_hole.lensing_radius.max(0.0),
            Color::srgba(1.0, 0.2, 0.2, 0.18),
            Color::srgba(0.4, 0.8, 1.0, 0.08),
        );
    }
}

#[cfg(feature = "client")]
#[derive(Component)]
struct BlackHoleVisual;

#[cfg(feature = "client")]
fn spawn_black_holes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    black_holes: Query<(Entity, &BlackHole), Added<BlackHole>>,
) {
    for (entity, black_hole) in &black_holes {
        let visual = commands
            .spawn((
                BlackHoleVisual,
                Mesh3d(meshes.add(bevy::math::primitives::Sphere::new(1.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::BLACK,
                    emissive: LinearRgba::BLACK,
                    unlit: true,
                    cull_mode: None,
                    ..default()
                })),
                Transform::from_scale(Vec3::splat(black_hole.event_horizon_radius.max(0.001))),
                Visibility::default(),
            ))
            .id();
        commands.entity(entity).add_child(visual);
    }
}

#[cfg(feature = "client")]
fn cleanup_black_holes(
    mut commands: Commands,
    visuals: Query<(Entity, &ChildOf), With<BlackHoleVisual>>,
    parents: Query<(), With<BlackHole>>,
) {
    for (entity, child_of) in &visuals {
        if parents.get(child_of.parent()).is_err() {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(feature = "client")]
fn sync_black_hole_visuals(
    parents: Query<(&BlackHole, &GlobalTransform)>,
    mut visuals: Query<(&ChildOf, &mut Transform), With<BlackHoleVisual>>,
) {
    for (child_of, mut transform) in &mut visuals {
        let Ok((black_hole, parent_transform)) = parents.get(child_of.parent()) else {
            continue;
        };
        let radius = black_hole.event_horizon_radius.max(0.001);
        let parent_scale = parent_transform.to_scale_rotation_translation().0;
        transform.scale = Vec3::new(
            safe_axis_scale(radius, parent_scale.x),
            safe_axis_scale(radius, parent_scale.y),
            safe_axis_scale(radius, parent_scale.z),
        );
    }
}

#[cfg(feature = "client")]
fn safe_axis_scale(radius: f32, parent_scale: f32) -> f32 {
    if parent_scale.abs() > 1e-4 {
        radius / parent_scale.abs()
    } else {
        radius
    }
}
