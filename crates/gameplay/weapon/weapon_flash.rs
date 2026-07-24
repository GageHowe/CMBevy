use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::{color::LinearRgba, pbr::MeshMaterial3d};

#[cfg(feature = "client")]
use crate::flash::{
    FlashMaterial, MIN_VISIBLE_BRIGHTNESS, MIN_VISIBLE_LIGHT_INTENSITY, flash_decay,
    spawn_flash_mesh, update_flash_material,
};

pub struct WeaponFlashPlugin;

impl Plugin for WeaponFlashPlugin {
    fn build(&self, _app: &mut App) {
        #[cfg(feature = "client")]
        _app.add_systems(Update, tick_weapon_flashes);
    }
}

#[cfg(feature = "client")]
#[derive(Component)]
pub struct WeaponFlash {
    pub color: LinearRgba,
    pub brightness: f32,
    pub brightness_decay: f32,
    pub light_intensity: f32,
    pub light_decay: f32,
    pub active: bool,
    pub age_secs: f32,
}

#[cfg(feature = "client")]
pub fn spawn_weapon_flash(
    world: &mut World,
    parent: Entity,
    local_offset: Vec3,
    scale: f32,
    color: Color,
    brightness: f32,
    brightness_decay: f32,
    light_intensity: f32,
    light_decay: f32,
    shadows_enabled: bool,
) -> Entity {
    let color = color.to_linear();
    let (mesh, material) = spawn_flash_mesh(world, color);
    let flash = world
        .spawn((
            WeaponFlash {
                color,
                brightness,
                brightness_decay,
                light_intensity,
                light_decay,
                active: false,
                age_secs: 0.0,
            },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            PointLight {
                intensity: 0.0,
                color: color.into(),
                range: scale * 8.0,
                shadow_maps_enabled: shadows_enabled,
                ..default()
            },
            Transform::from_translation(local_offset).with_scale(Vec3::splat(scale)),
        ))
        .id();
    world.entity_mut(parent).add_child(flash);
    flash
}

#[cfg(feature = "client")]
pub fn trigger_weapon_flash(world: &mut World, entity: Entity) {
    let Some(mut flash) = world.get_mut::<WeaponFlash>(entity) else {
        return;
    };
    flash.active = true;
    flash.age_secs = 0.0;
}

#[cfg(feature = "client")]
fn tick_weapon_flashes(
    time: Res<Time>,
    mut flashes: Query<(
        &mut WeaponFlash,
        &MeshMaterial3d<FlashMaterial>,
        &mut PointLight,
    )>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut materials: ResMut<Assets<FlashMaterial>>,
) {
    let dt = time.delta_secs();
    let camera_pos = camera
        .single()
        .map(GlobalTransform::translation)
        .unwrap_or(Vec3::ZERO);
    for (mut flash, material_handle, mut light) in &mut flashes {
        if !flash.active {
            light.intensity = 0.0;
            update_flash_material(
                material_handle,
                &mut materials,
                flash.color,
                0.0,
                flash.brightness,
                camera_pos,
            );
            continue;
        }
        flash.age_secs += dt;
        let brightness = flash_decay(flash.brightness, flash.brightness_decay, flash.age_secs);
        let light_intensity = flash_decay(flash.light_intensity, flash.light_decay, flash.age_secs);
        if brightness <= MIN_VISIBLE_BRIGHTNESS && light_intensity <= MIN_VISIBLE_LIGHT_INTENSITY {
            flash.active = false;
            light.intensity = 0.0;
            update_flash_material(
                material_handle,
                &mut materials,
                flash.color,
                0.0,
                flash.brightness,
                camera_pos,
            );
            continue;
        }
        light.intensity = light_intensity;
        light.color = flash.color.into();
        update_flash_material(
            material_handle,
            &mut materials,
            flash.color,
            brightness,
            flash.brightness,
            camera_pos,
        );
    }
}
