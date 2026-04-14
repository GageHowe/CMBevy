use bevy::prelude::*;
use hanabi::prelude::{EffectAsset, EffectMaterial};

use crate::helpers::{burst_effect, spawn_one_shot_effect};

#[derive(Resource)]
pub struct RpgExplosionEffect {
    sparks: Handle<EffectAsset>,
    smoke: Handle<EffectAsset>,
    smoke_texture: Handle<Image>,
}

impl FromWorld for RpgExplosionEffect {
    fn from_world(world: &mut World) -> Self {
        let sparks = burst_effect(
            "rpg_explosion_sparks",
            96,                // capacity
            24.0,              // burst_count
            (0.3, 0.8),        // lifetime
            (16.0, 64.0),      // speed
            5.0,               // drag
            Vec3::splat(0.08), // size
            Some((0.75, 1.25)),
            false,
            false,
            false,
            &[
                (0.0, Vec4::new(10.0, 6.0, 1.4, 1.0)),
                (0.5, Vec4::new(4.0, 1.5, 0.3, 0.5)),
                (1.0, Vec4::new(0.8, 0.2, 0.05, 0.0)),
            ],
        );
        let smoke = burst_effect(
            "rpg_explosion_smoke",
            96,
            20.0,
            (1.0, 3.0),
            (4.0, 20.0),
            5.0,
            Vec3::splat(12.0),
            Some((0.6, 1.6)),
            true,
            true,
            true,
            &[
                (0.0, Vec4::new(1.0, 0.9, 0.8, 0.65)),
                (0.4, Vec4::new(0.5, 0.5, 0.5, 0.22)),
                (1.0, Vec4::new(0.2, 0.2, 0.2, 0.0)),
            ],
        );
        let smoke_texture =
            world.resource::<AssetServer>().load("textures/particles/smoke_06_a.png");
        let mut effects = world.resource_mut::<Assets<EffectAsset>>();
        Self { sparks: effects.add(sparks), smoke: effects.add(smoke), smoke_texture }
    }
}

pub fn spawn_rpg_explosion_effect(world: &mut World, position: Vec3, inherit_velocity: Vec3) {
    let (sparks, smoke, smoke_texture) = {
        let effect = world.resource::<RpgExplosionEffect>();
        (effect.sparks.clone(), effect.smoke.clone(), effect.smoke_texture.clone())
    };
    spawn_one_shot_effect(
        world,
        "rpg_explosion_sparks_effect",
        sparks,
        None,
        position,
        inherit_velocity,
        0.4,
    );
    spawn_one_shot_effect(
        world,
        "rpg_explosion_smoke_effect",
        smoke,
        Some(EffectMaterial { images: vec![smoke_texture] }),
        position,
        inherit_velocity,
        3.5,
    );
}
