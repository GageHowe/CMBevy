use bevy::prelude::{Vec3, Vec4};
use hanabi::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScalarRangeDef {
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ColorKeyDef {
    pub time: f32,
    pub value: (f32, f32, f32, f32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocityBurstEffectDef {
    pub name: String,
    pub capacity: u32,
    pub burst_count: f32,
    pub lifetime: ScalarRangeDef,
    pub speed: ScalarRangeDef,
    pub drag: f32,
    pub size: (f32, f32, f32),
    pub camera_facing: bool,
    pub colors: Vec<ColorKeyDef>,
}

impl VelocityBurstEffectDef {
    pub fn from_ron_str(source: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(source)
    }

    pub fn to_effect_asset(&self) -> EffectAsset {
        let writer = ExprWriter::new();
        let inherit_velocity = writer.add_property("inherit_velocity", Vec3::ZERO.into());
        let inherit_velocity = writer.prop(inherit_velocity);
        let spread = writer
            .rand(VectorType::VEC3F)
            .mul(writer.lit(2.0))
            .sub(writer.lit(1.0))
            .normalized();
        let speed = writer
            .lit(self.speed.min)
            .uniform(writer.lit(self.speed.max));
        let init_velocity =
            SetAttributeModifier::new(Attribute::VELOCITY, (inherit_velocity + spread * speed).expr());
        let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0).expr());
        let init_lifetime = SetAttributeModifier::new(
            Attribute::LIFETIME,
            writer
                .lit(self.lifetime.min)
                .uniform(writer.lit(self.lifetime.max))
                .expr(),
        );
        let init_pos =
            SetAttributeModifier::new(Attribute::POSITION, writer.lit(Vec3::ZERO).expr());
        let update_drag = LinearDragModifier::new(writer.lit(self.drag).expr());

        let mut color_gradient = hanabi::prelude::Gradient::new();
        for key in &self.colors {
            color_gradient.add_key(key.time, Vec4::new(key.value.0, key.value.1, key.value.2, key.value.3));
        }

        let mut effect = EffectAsset::new(
            self.capacity,
            SpawnerSettings::burst(self.burst_count.into(), 1.0.into()),
            writer.finish(),
        )
        .with_name(self.name.clone())
        .init(init_pos)
        .init(init_velocity)
        .init(init_age)
        .init(init_lifetime)
        .update(update_drag)
        .render(ColorOverLifetimeModifier::new(color_gradient))
        .render(SizeOverLifetimeModifier {
            gradient: hanabi::prelude::Gradient::constant(Vec3::new(self.size.0, self.size.1, self.size.2)),
            screen_space_size: false,
        });

        if self.camera_facing {
            effect = effect.render(OrientModifier::new(OrientMode::FaceCameraPosition));
        }

        effect
    }
}
