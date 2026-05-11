use bevy::prelude::*;
use hanabi::prelude::{EffectAsset, EffectMaterial};

use crate::helpers::{burst_effect, spawn_one_shot_effect};

#[derive(Resource)]
pub struct LobberExplosionEffect {
    sparks: Handle<EffectAsset>,
    smoke: Handle<EffectAsset>,
    smoke_texture: Handle<Image>,
}

#[derive(Resource)]
pub struct ThumperExplosionEffect {
    sparks: Handle<EffectAsset>,
    smoke: Handle<EffectAsset>,
    smoke_texture: Handle<Image>,
}

#[derive(Resource)]
pub struct SpaceshipDeathExplosionEffect {
    sparks: Handle<EffectAsset>,
    smoke: Handle<EffectAsset>,
    smoke_texture: Handle<Image>,
}

impl FromWorld for LobberExplosionEffect {
    fn from_world(world: &mut World) -> Self {
        let sparks = burst_effect(
            "lobber_explosion_sparks",
            96,
            24.0,
            (0.3, 0.8),
            (16.0, 64.0),
            5.0,
            Vec3::splat(0.08),
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
            "lobber_explosion_smoke",
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
        let smoke_texture = world
            .resource::<AssetServer>()
            .load("textures/particles/smoke_06_a.png");
        let mut effects = world.resource_mut::<Assets<EffectAsset>>();
        Self {
            sparks: effects.add(sparks),
            smoke: effects.add(smoke),
            smoke_texture,
        }
    }
}

impl FromWorld for ThumperExplosionEffect {
    fn from_world(world: &mut World) -> Self {
        let sparks = burst_effect(
            "thumper_explosion_sparks",
            64,
            14.0,
            (0.2, 0.55),
            (10.0, 34.0),
            6.0,
            Vec3::splat(0.05),
            Some((0.65, 1.0)),
            false,
            false,
            false,
            &[
                (0.0, Vec4::new(4.0, 5.5, 7.5, 0.9)),
                (0.5, Vec4::new(1.2, 2.0, 2.8, 0.35)),
                (1.0, Vec4::new(0.2, 0.4, 0.8, 0.0)),
            ],
        );
        let smoke = burst_effect(
            "thumper_explosion_smoke",
            48,
            10.0,
            (0.45, 1.3),
            (2.0, 10.0),
            6.0,
            Vec3::splat(6.0),
            Some((0.45, 0.9)),
            true,
            true,
            true,
            &[
                (0.0, Vec4::new(0.65, 0.72, 0.78, 0.18)),
                (0.4, Vec4::new(0.35, 0.42, 0.48, 0.08)),
                (1.0, Vec4::new(0.18, 0.22, 0.26, 0.0)),
            ],
        );
        let smoke_texture = world
            .resource::<AssetServer>()
            .load("textures/particles/smoke_06_a.png");
        let mut effects = world.resource_mut::<Assets<EffectAsset>>();
        Self {
            sparks: effects.add(sparks),
            smoke: effects.add(smoke),
            smoke_texture,
        }
    }
}

impl FromWorld for SpaceshipDeathExplosionEffect {
    fn from_world(world: &mut World) -> Self {
        let sparks = burst_effect(
            "spaceship_death_sparks",
            256,
            72.0,
            (0.45, 1.4),
            (22.0, 95.0),
            4.5,
            Vec3::splat(0.16),
            Some((0.7, 1.6)),
            false,
            false,
            false,
            &[
                (0.0, Vec4::new(16.0, 8.0, 2.0, 1.0)),
                (0.45, Vec4::new(6.0, 2.0, 0.4, 0.55)),
                (1.0, Vec4::new(1.2, 0.25, 0.05, 0.0)),
            ],
        );
        let smoke = burst_effect(
            "spaceship_death_smoke",
            192,
            42.0,
            (2.0, 5.5),
            (6.0, 28.0),
            3.0,
            Vec3::splat(22.0),
            Some((0.8, 2.1)),
            true,
            true,
            true,
            &[
                (0.0, Vec4::new(1.0, 0.95, 0.85, 0.8)),
                (0.35, Vec4::new(0.55, 0.55, 0.55, 0.32)),
                (1.0, Vec4::new(0.18, 0.18, 0.18, 0.0)),
            ],
        );
        let smoke_texture = world
            .resource::<AssetServer>()
            .load("textures/particles/smoke_06_a.png");
        let mut effects = world.resource_mut::<Assets<EffectAsset>>();
        Self {
            sparks: effects.add(sparks),
            smoke: effects.add(smoke),
            smoke_texture,
        }
    }
}

pub fn spawn_lobber_explosion_effect(world: &mut World, position: Vec3, inherit_velocity: Vec3) {
    let (sparks, smoke, smoke_texture) = {
        let effect = world.resource::<LobberExplosionEffect>();
        (
            effect.sparks.clone(),
            effect.smoke.clone(),
            effect.smoke_texture.clone(),
        )
    };
    spawn_one_shot_effect(
        world,
        "lobber_explosion_sparks_effect",
        sparks,
        None,
        position,
        inherit_velocity,
        0.4,
        1.0,
    );
    spawn_one_shot_effect(
        world,
        "lobber_explosion_smoke_effect",
        smoke,
        Some(EffectMaterial {
            images: vec![smoke_texture],
        }),
        position,
        inherit_velocity,
        3.5,
        1.0,
    );
}

pub fn spawn_thumper_explosion_effect(world: &mut World, position: Vec3, inherit_velocity: Vec3) {
    let (sparks, smoke, smoke_texture) = {
        let effect = world.resource::<ThumperExplosionEffect>();
        (
            effect.sparks.clone(),
            effect.smoke.clone(),
            effect.smoke_texture.clone(),
        )
    };
    spawn_one_shot_effect(
        world,
        "thumper_explosion_sparks_effect",
        sparks,
        None,
        position,
        inherit_velocity,
        0.25,
        1.0,
    );
    spawn_one_shot_effect(
        world,
        "thumper_explosion_smoke_effect",
        smoke,
        Some(EffectMaterial {
            images: vec![smoke_texture],
        }),
        position,
        inherit_velocity,
        1.1,
        1.0,
    );
}

pub fn spawn_spaceship_death_explosion_effect(
    world: &mut World,
    position: Vec3,
    inherit_velocity: Vec3,
) {
    let (sparks, smoke, smoke_texture) = {
        let effect = world.resource::<SpaceshipDeathExplosionEffect>();
        (
            effect.sparks.clone(),
            effect.smoke.clone(),
            effect.smoke_texture.clone(),
        )
    };
    spawn_one_shot_effect(
        world,
        "spaceship_death_sparks_effect",
        sparks,
        None,
        position,
        inherit_velocity,
        1.2,
        1.0,
    );
    spawn_one_shot_effect(
        world,
        "spaceship_death_smoke_effect",
        smoke,
        Some(EffectMaterial {
            images: vec![smoke_texture],
        }),
        position,
        inherit_velocity,
        6.0,
        1.0,
    );
}
