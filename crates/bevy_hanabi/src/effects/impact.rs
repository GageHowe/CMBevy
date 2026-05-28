use bevy::prelude::*;
use hanabi::prelude::{EffectAsset, EffectMaterial};

use crate::helpers::{burst_effect, spawn_one_shot_effect};

#[derive(Resource)]
pub struct DustImpactEffect {
    smoke: Handle<EffectAsset>,
    smoke_texture: Handle<Image>,
}

#[derive(Resource)]
pub struct SparksImpactEffect {
    sparks: Handle<EffectAsset>,
}

impl FromWorld for DustImpactEffect {
    fn from_world(world: &mut World) -> Self {
        let smoke = burst_effect(
            "dust_impact_smoke",
            48,
            12.0,
            (0.35, 1.1),
            (1.5, 6.0),
            6.0,
            Vec3::splat(1.0),
            Some((0.35, 0.8)),
            true,
            true,
            true,
            &[
                (0.0, Vec4::new(0.8, 0.72, 0.58, 0.2)),
                (0.4, Vec4::new(0.55, 0.48, 0.38, 0.1)),
                (1.0, Vec4::new(0.35, 0.32, 0.28, 0.0)),
            ],
        );
        let smoke_texture = world
            .resource::<AssetServer>()
            .load("textures/particles/smoke_06_a.png");
        let mut effects = world.resource_mut::<Assets<EffectAsset>>();
        Self {
            smoke: effects.add(smoke),
            smoke_texture,
        }
    }
}

impl FromWorld for SparksImpactEffect {
    fn from_world(world: &mut World) -> Self {
        let sparks = burst_effect(
            "impact_sparks",
            64,
            10.0,
            (0.15, 0.45),
            (10.0, 30.0),
            8.0,
            Vec3::splat(0.04),
            Some((0.6, 1.0)),
            false,
            false,
            false,
            &[
                (0.0, Vec4::new(8.0, 6.0, 2.0, 0.8)),
                (0.5, Vec4::new(2.5, 1.2, 0.25, 0.25)),
                (1.0, Vec4::new(0.5, 0.15, 0.04, 0.0)),
            ],
        );
        let mut effects = world.resource_mut::<Assets<EffectAsset>>();
        Self {
            sparks: effects.add(sparks),
        }
    }
}

pub fn spawn_dust_impact_effect(world: &mut World, position: Vec3, inherit_velocity: Vec3) {
    let (smoke, smoke_texture) = {
        let effect = world.resource::<DustImpactEffect>();
        (effect.smoke.clone(), effect.smoke_texture.clone())
    };
    spawn_one_shot_effect(
        world,
        "dust_impact_effect",
        smoke,
        Some(EffectMaterial {
            images: vec![smoke_texture],
        }),
        position,
        inherit_velocity,
        1.0,
    );
}

pub fn spawn_sparks_impact_effect(world: &mut World, position: Vec3, inherit_velocity: Vec3) {
    let sparks = world.resource::<SparksImpactEffect>().sparks.clone();
    spawn_one_shot_effect(
        world,
        "sparks_impact_effect",
        sparks,
        None,
        position,
        inherit_velocity,
        0.35,
    );
}
