use bevy::prelude::*;
use hanabi::prelude::*;

pub fn burst_effect(
    name: &str,
    capacity: u32,
    burst_count: f32,
    lifetime: (f32, f32),
    speed: (f32, f32),
    drag: f32,
    size: Vec3,
    random_size_range: Option<(f32, f32)>,
    camera_facing: bool,
    textured: bool,
    random_rotation: bool,
    colors: &[(f32, Vec4)],
) -> EffectAsset {
    let writer = ExprWriter::new();

    let mut color_gradient = hanabi::prelude::Gradient::new();
    for (time, value) in colors {
        color_gradient.add_key(*time, *value);
    }

    let init_pos = SetAttributeModifier::new(Attribute::POSITION, writer.lit(Vec3::ZERO).expr());
    let init_vel = SetAttributeModifier::new(
        Attribute::VELOCITY,
        (writer.prop(writer.add_property("inherit_velocity", Vec3::ZERO.into()))
            + writer
                .rand(VectorType::VEC3F)
                .mul(writer.lit(2.0))
                .sub(writer.lit(1.0))
                .normalized()
                * writer.lit(speed.0).uniform(writer.lit(speed.1)))
        .expr(),
    );
    let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0).expr());
    let init_lifetime = SetAttributeModifier::new(
        Attribute::LIFETIME,
        writer.lit(lifetime.0).uniform(writer.lit(lifetime.1)).expr(),
    );
    let init_rotation = SetAttributeModifier::new(
        Attribute::F32_0,
        (writer.rand(ScalarType::Float) * writer.lit(std::f32::consts::TAU)).expr(),
    );
    let init_size = random_size_range.map(|(min_size, max_size)| {
        SetAttributeModifier::new(
            Attribute::SIZE,
            (writer.rand(ScalarType::Float) * writer.lit(max_size - min_size)
                + writer.lit(min_size))
            .expr(),
        )
    });
    let rotation_attr = writer.attr(Attribute::F32_0).expr();
    let drag_modifier = LinearDragModifier::new(writer.lit(drag).expr());
    let size_modifier = SizeOverLifetimeModifier {
        gradient: hanabi::prelude::Gradient::constant(size),
        screen_space_size: false,
    };

    let texture_slot = writer.lit(0u32).expr();
    let mut module = writer.finish();
    if textured {
        module.add_texture_slot("color");
    }

    let mut effect = EffectAsset::new(capacity, SpawnerSettings::once(burst_count.into()), module)
        .with_name(name.to_string())
        .init(init_pos)
        .init(init_vel)
        .init(init_age)
        .init(init_lifetime)
        .update(drag_modifier)
        .render(ColorOverLifetimeModifier::new(color_gradient))
        .render(size_modifier);

    if random_rotation {
        effect = effect.init(init_rotation);
    }
    if let Some(init_size) = init_size {
        effect = effect.init(init_size);
    }

    if textured {
        effect = effect.render(ParticleTextureModifier {
            texture_slot,
            sample_mapping: ImageSampleMapping::Modulate,
        });
    }

    if camera_facing {
        if random_rotation {
            effect = effect.render(OrientModifier {
                mode: OrientMode::FaceCameraPosition,
                rotation: Some(rotation_attr),
            });
        } else {
            effect = effect.render(OrientModifier::new(OrientMode::FaceCameraPosition));
        }
    }

    effect
}

#[derive(Component)]
pub struct OneShotEffect {
    pub remaining: f32,
}

pub fn spawn_one_shot_effect(
    world: &mut World,
    name: &'static str,
    handle: Handle<EffectAsset>,
    material: Option<EffectMaterial>,
    position: Vec3,
    inherit_velocity: Vec3,
    remaining: f32,
) {
    let mut properties = EffectProperties::default();
    properties.set("inherit_velocity", inherit_velocity.into());
    let mut entity = world.spawn((
        Name::new(name),
        Transform::from_translation(position),
        ParticleEffect::new(handle),
        properties,
        OneShotEffect { remaining },
    ));
    if let Some(material) = material {
        entity.insert(material);
    }
}

pub fn tick_one_shot_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut OneShotEffect)>,
) {
    for (entity, mut one_shot) in &mut q {
        one_shot.remaining -= time.delta_secs();
        if one_shot.remaining <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
