use bevy::prelude::*;
use gameplay::{
    health::Health,
    pawn::{Possessed, WeaponSlots},
    reticle::{AimOrigin, AimReticle, default_crosshair_path},
};
use physics::physics_world::{PhysicsWorld, rb_vel};

use crate::settings::Settings;

#[derive(Component)]
pub struct PredictionReticle;

#[derive(Component)]
pub struct Crosshair;

const CROSSHAIR_SIZE: f32 = 32.0;
const PREDICTION_RETICLE_SIZE: f32 = 18.0;

pub fn spawn_prediction_reticle(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    settings: Option<Res<Settings>>,
) {
    let size = scaled_reticle_size(PREDICTION_RETICLE_SIZE, reticle_scale(settings.as_deref()));
    commands.spawn((
        PredictionReticle,
        ImageNode {
            image: asset_server.load("textures/crosshairs/crosshair030.png"),
            color: Color::srgba(1.0, 1.0, 1.0, 0.2),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(size),
            height: Val::Px(size),
            ..default()
        },
        ZIndex(10),
        Visibility::Hidden,
    ));
}

pub fn update_prediction_reticle(
    pawn: Query<
        (
            Entity,
            Option<&AimReticle>,
            Option<&AimOrigin>,
            Option<&WeaponSlots>,
        ),
        With<Possessed>,
    >,
    transforms: Query<&GlobalTransform>,
    weapons: Query<&AimReticle>,
    targets: Query<(Entity, &GlobalTransform, &Health), Without<Possessed>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    world: Res<PhysicsWorld>,
    settings: Option<Res<Settings>>,
    mut indicator: Query<(&mut Node, &mut Visibility), With<PredictionReticle>>,
) {
    let Ok((mut node, mut vis)) = indicator.single_mut() else {
        return;
    };
    let half_size =
        scaled_reticle_size(PREDICTION_RETICLE_SIZE, reticle_scale(settings.as_deref())) * 0.5;
    let show = (|| -> Option<Vec2> {
        let (pawn_entity, possessed_reticle, aim_origin, slots) = pawn.single().ok()?;
        let projectile_speed = reticle_for_possessed(possessed_reticle, slots, &weapons)?.1?;
        let origin_entity = aim_origin.map_or(pawn_entity, |aim_origin| aim_origin.0);
        let origin = transforms.get(origin_entity).ok()?.translation();
        let (camera, camera_gt) = camera.single().ok()?;
        let viewport_size = camera.logical_viewport_size()?;
        let viewport_center = viewport_size * 0.5;
        let shooter_velocity = world
            .entity_to_handle
            .get(&pawn_entity)
            .and_then(|&h| world.rigid_body_set.get(h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);

        let mut best = None;
        let mut best_score = f32::INFINITY;
        for (target_entity, target_gt, health) in targets.iter() {
            if health.current <= 0 {
                continue;
            }
            let target_pos = target_gt.translation();
            let Ok(screen_pos) = camera.world_to_viewport(camera_gt, target_pos) else {
                continue;
            };
            if world
                .cast_ray(
                    origin,
                    (target_pos - origin).normalize_or_zero(),
                    origin.distance(target_pos),
                    &[pawn_entity],
                )
                .is_some_and(|(hit, _)| hit != target_entity)
            {
                continue;
            }
            let score = screen_pos.distance_squared(viewport_center);
            if score >= best_score {
                continue;
            }
            let target_velocity = world
                .entity_to_handle
                .get(&target_entity)
                .and_then(|&h| world.rigid_body_set.get(h))
                .map(rb_vel)
                .unwrap_or(Vec3::ZERO);
            let relative_position = target_pos - origin;
            let relative_velocity = target_velocity - shooter_velocity;
            let Some(time) =
                solve_intercept_time(relative_position, relative_velocity, projectile_speed)
            else {
                continue;
            };
            let relative_intercept = relative_position + relative_velocity * time;
            let aim_point = origin + relative_intercept;
            let Ok(intercept_screen) = camera.world_to_viewport(camera_gt, aim_point) else {
                continue;
            };
            best_score = score;
            best = Some(intercept_screen);
        }
        best
    })();
    match show {
        Some(pos) => {
            node.left = Val::Px(pos.x - half_size);
            node.top = Val::Px(pos.y - half_size);
            *vis = Visibility::Inherited;
        }
        None => *vis = Visibility::Hidden,
    }
}

fn solve_intercept_time(
    relative_position: Vec3,
    relative_velocity: Vec3,
    speed: f32,
) -> Option<f32> {
    let a = relative_velocity.length_squared() - speed * speed;
    let b = 2.0 * relative_position.dot(relative_velocity);
    let c = relative_position.length_squared();
    if c <= f32::EPSILON {
        return Some(0.0);
    }
    if a.abs() <= f32::EPSILON {
        if b.abs() <= f32::EPSILON {
            return None;
        }
        let t = -c / b;
        return (t > 0.0).then_some(t);
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let t0 = (-b - root) / (2.0 * a);
    let t1 = (-b + root) / (2.0 * a);
    [t0, t1]
        .into_iter()
        .filter(|t| *t > 0.0 && t.is_finite())
        .min_by(f32::total_cmp)
}

pub fn update_reticle(
    possessed: Query<(Option<&AimReticle>, Option<&WeaponSlots>), With<Possessed>>,
    reticles: Query<&AimReticle>,
    mut crosshair: Query<&mut ImageNode, With<Crosshair>>,
    asset_server: Res<AssetServer>,
    mut current: Local<Option<&'static str>>,
) {
    let path = possessed
        .single()
        .ok()
        .and_then(|(possessed_reticle, slots)| {
            reticle_for_possessed(possessed_reticle, slots, &reticles)
        })
        .map(|reticle| reticle.0)
        .unwrap_or(default_crosshair_path());
    if *current == Some(path) {
        return;
    }
    if let Ok(mut img) = crosshair.single_mut() {
        img.image = asset_server.load(path);
        *current = Some(path);
    }
}

pub fn spawn_crosshair(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    settings: Option<Res<Settings>>,
) {
    let size = scaled_reticle_size(CROSSHAIR_SIZE, reticle_scale(settings.as_deref()));
    commands.spawn((
        Crosshair,
        ImageNode::new(asset_server.load(default_crosshair_path())),
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            margin: UiRect {
                left: Val::Px(-size * 0.5),
                top: Val::Px(-size * 0.5),
                ..default()
            },
            ..default()
        },
    ));
}

pub fn apply_reticle_scale(
    settings: Res<Settings>,
    mut crosshair: Query<&mut Node, (With<Crosshair>, Without<PredictionReticle>)>,
    mut prediction: Query<&mut Node, (With<PredictionReticle>, Without<Crosshair>)>,
) {
    let crosshair_size = scaled_reticle_size(CROSSHAIR_SIZE, settings.reticle_scale);
    let prediction_size = scaled_reticle_size(PREDICTION_RETICLE_SIZE, settings.reticle_scale);

    if let Ok(mut node) = crosshair.single_mut() {
        set_reticle_size(&mut node, crosshair_size);
        node.margin.left = Val::Px(-crosshair_size * 0.5);
        node.margin.top = Val::Px(-crosshair_size * 0.5);
    }
    if let Ok(mut node) = prediction.single_mut() {
        set_reticle_size(&mut node, prediction_size);
    }
}

fn reticle_for_possessed<'a>(
    possessed_reticle: Option<&'a AimReticle>,
    slots: Option<&WeaponSlots>,
    reticles: &'a Query<&AimReticle>,
) -> Option<&'a AimReticle> {
    possessed_reticle.or_else(|| {
        let weapon_entity = slots?.active().1?;
        reticles.get(weapon_entity).ok()
    })
}

fn scaled_reticle_size(base_size: f32, scale: f32) -> f32 {
    base_size * scale.clamp(0.5, 2.0)
}

fn reticle_scale(settings: Option<&Settings>) -> f32 {
    settings.map_or(1.0, |settings| settings.reticle_scale)
}

fn set_reticle_size(node: &mut Node, size: f32) {
    node.width = Val::Px(size);
    node.height = Val::Px(size);
}
