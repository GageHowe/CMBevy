use bevy::prelude::*;
use hanabi::prelude::{
    Attribute, ColorOverLifetimeModifier, EffectAsset, ExprWriter, OrientMode, OrientModifier,
    ParticleEffect, SetAttributeModifier, SimulationSpace, SizeOverLifetimeModifier,
    SpawnerSettings,
};

use crate::helpers::OneShotEffect;

const JETPACK_OFFSET: Vec3 = Vec3::new(0.0, -0.7, 0.0);
const DASH_OFFSET: f32 = 0.35;

#[derive(Resource)]
pub struct AbilityEffects {
    jetpack: Handle<EffectAsset>,
    dash: Handle<EffectAsset>,
}

impl FromWorld for AbilityEffects {
    fn from_world(world: &mut World) -> Self {
        let mut effects = world.resource_mut::<Assets<EffectAsset>>();
        Self {
            jetpack: effects.add(local_directional_effect(
                "jetpack_effect",
                256,
                SpawnerSettings::rate(48.0.into()),
                (0.15, 0.35),
                (9.0, 18.0),
                Vec3::splat(0.06),
                Vec3::NEG_Y,
                0.35,
                &[
                    (0.0, Vec4::new(4.0, 4.0, 2.0, 0.8)),
                    // (0.5, Vec4::new(0.4, 0.8, 2.0, 0.25)),
                    (1.0, Vec4::new(2.0, 1.0, 0.1, 0.0)),
                ],
            )),
            dash: effects.add(local_directional_effect(
                "dash_effect",
                128,
                SpawnerSettings::once(22.0.into()),
                (0.15, 0.35),
                (10.0, 28.0),
                Vec3::splat(0.08),
                Vec3::NEG_Y,
                0.45,
                &[
                    (0.0, Vec4::new(4.0, 3.0, 0.8, 0.75)),
                    (0.6, Vec4::new(1.5, 1.0, 0.3, 0.2)),
                    (1.0, Vec4::new(0.4, 0.2, 0.05, 0.0)),
                ],
            )),
        }
    }
}

fn local_directional_effect(
    name: &str,
    capacity: u32,
    spawner: SpawnerSettings,
    lifetime: (f32, f32),
    speed: (f32, f32),
    size: Vec3,
    direction: Vec3,
    spread: f32,
    colors: &[(f32, Vec4)],
) -> EffectAsset {
    let writer = ExprWriter::new();
    let jitter = writer
        .rand(hanabi::prelude::VectorType::VEC3F)
        .mul(writer.lit(2.0 * spread))
        .sub(writer.lit(spread));
    let velocity = ((writer.lit(direction) + jitter).normalized()
        * writer.lit(speed.0).uniform(writer.lit(speed.1)))
    .expr();
    base_effect(
        name, capacity, spawner, writer, lifetime, size, colors, velocity,
    )
    .with_simulation_space(SimulationSpace::Local)
}

fn base_effect(
    name: &str,
    capacity: u32,
    spawner: SpawnerSettings,
    writer: ExprWriter,
    lifetime: (f32, f32),
    size: Vec3,
    colors: &[(f32, Vec4)],
    velocity: hanabi::prelude::ExprHandle,
) -> EffectAsset {
    let mut color_gradient = hanabi::prelude::Gradient::new();
    for (time, value) in colors {
        color_gradient.add_key(*time, *value);
    }

    let init_pos = SetAttributeModifier::new(Attribute::POSITION, writer.lit(Vec3::ZERO).expr());
    let init_vel = SetAttributeModifier::new(Attribute::VELOCITY, velocity);
    let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0).expr());
    let init_lifetime = SetAttributeModifier::new(
        Attribute::LIFETIME,
        writer
            .lit(lifetime.0)
            .uniform(writer.lit(lifetime.1))
            .expr(),
    );

    EffectAsset::new(capacity, spawner, writer.finish())
        .with_name(name.to_string())
        .init(init_pos)
        .init(init_vel)
        .init(init_age)
        .init(init_lifetime)
        .render(ColorOverLifetimeModifier::new(color_gradient))
        .render(SizeOverLifetimeModifier {
            gradient: hanabi::prelude::Gradient::constant(size),
            screen_space_size: false,
        })
        .render(OrientModifier::new(OrientMode::FaceCameraPosition))
}

pub fn spawn_jetpack_effect(world: &mut World) -> Entity {
    let handle = world.resource::<AbilityEffects>().jetpack.clone();
    world
        .spawn((
            Name::new("jetpack_effect"),
            Transform::from_translation(JETPACK_OFFSET),
            ParticleEffect::new(handle),
        ))
        .id()
}

pub fn spawn_dash_effect(world: &mut World, owner: Entity, local_emit_dir: Vec3) {
    let handle = world.resource::<AbilityEffects>().dash.clone();
    let emit_dir = if local_emit_dir.length_squared() > 1e-6 {
        local_emit_dir.normalize()
    } else {
        Vec3::NEG_Y
    };
    let fx = world
        .spawn((
            Name::new("dash_effect"),
            Transform {
                translation: emit_dir * DASH_OFFSET,
                rotation: Quat::from_rotation_arc(Vec3::NEG_Y, emit_dir),
                ..default()
            },
            ParticleEffect::new(handle),
            OneShotEffect { remaining: 0.4 },
        ))
        .id();
    if let Ok(mut owner_entity) = world.get_entity_mut(owner) {
        owner_entity.add_child(fx);
    }
}
