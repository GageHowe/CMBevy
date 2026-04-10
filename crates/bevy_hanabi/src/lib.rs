use bevy::prelude::*;

pub mod effect_def;

pub mod prelude {
    pub use crate::effect_def::*;
    pub use crate::spawn_rpg_explosion_effect;
    pub use crate::HanabiEffectsPlugin;
    pub use hanabi::prelude::*;
}

pub struct HanabiEffectsPlugin;
impl Plugin for HanabiEffectsPlugin {
    fn build(&self, app: &mut App) {
        // Keep the integration point small so effects can live here without leaking Hanabi setup
        // into the client binary or gameplay crates.
        app.add_plugins(hanabi::prelude::HanabiPlugin)
            .init_resource::<ExplosionEffects>()
            .add_systems(Update, tick_one_shot_effects);
    }
}

#[derive(Resource)]
struct ExplosionEffects {
    rpg: Handle<hanabi::prelude::EffectAsset>,
}

impl FromWorld for ExplosionEffects {
    fn from_world(world: &mut World) -> Self {
        let effect = effect_def::VelocityBurstEffectDef::from_ron_str(RPG_EXPLOSION_EFFECT)
            .expect("valid built-in RPG Hanabi effect")
            .to_effect_asset();
        let handle = world
            .resource_mut::<Assets<hanabi::prelude::EffectAsset>>()
            .add(effect);
        Self { rpg: handle }
    }
}

#[derive(Component)]
struct OneShotEffect {
    remaining: f32,
}

pub fn spawn_rpg_explosion_effect(world: &mut World, position: Vec3, inherit_velocity: Vec3) {
    let handle = world.resource::<ExplosionEffects>().rpg.clone();
    let mut properties = hanabi::prelude::EffectProperties::default();
    properties.set("inherit_velocity", inherit_velocity.into());
    // Keep lifetime cleanup here so gameplay code only provides effect inputs.
    world.spawn((
        Name::new("rpg_explosion_effect"),
        Transform::from_translation(position),
        hanabi::prelude::ParticleEffect::new(handle),
        properties,
        OneShotEffect { remaining: 1.0 },
    ));
}

fn tick_one_shot_effects(
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

const RPG_EXPLOSION_EFFECT: &str = r#"
(
    name: "rpg_explosion",
    capacity: 96,
    burst_count: 40.0,
    lifetime: (min: 0.65, max: 0.8125),
    speed: (min: 5.0, max: 16.0),
    drag: 6.0,
    size: (0.45, 0.45, 0.45),
    camera_facing: true,
    colors: [
        (time: 0.0, value: (6.0, 3.0, 0.8, 1.0)),
        (time: 0.35, value: (2.5, 0.7, 0.15, 0.6)),
        (time: 1.0, value: (0.3, 0.3, 0.3, 0.0)),
    ],
)
"#;
