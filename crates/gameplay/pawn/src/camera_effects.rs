use bevy::prelude::*;
#[cfg(feature = "client")]
use noise_functions::{Noise, Perlin};

/// spring-damping recoil + procedural shake + zoom applied on top of gameplay aim.
/// Placed on the Camera3d entity by biped possession; any pawn system can write to it.
#[derive(Component)]
pub struct CameraEffector {
    pub pitch_offset: f32,
    pub pitch_vel: f32,
    pub yaw_offset: f32,
    pub yaw_vel: f32,
    pub base_translation: Vec3,
    /// for recoil recovery
    pub recovery_speed: f32,
    /// User's base FOV in degrees. Set at possession; updated when settings change.
    pub base_fov: f32,
    /// Zoom multiplier for this tick (1.0 = no zoom). Weapons write this; no auto-reset.
    pub zoom_multiplier: f32,
    /// Smoothly lerped FOV in degrees, written to Projection each frame.
    pub current_fov: f32,
    #[cfg(feature = "client")]
    pub(crate) active_shakes: Vec<ActiveCameraShake>,
}

impl Default for CameraEffector {
    fn default() -> Self {
        Self {
            pitch_offset: 0.0,
            pitch_vel: 0.0,
            yaw_offset: 0.0,
            yaw_vel: 0.0,
            base_translation: Vec3::ZERO,
            recovery_speed: 18.0,
            base_fov: 90.0,
            zoom_multiplier: 1.0,
            current_fov: 90.0,
            #[cfg(feature = "client")]
            active_shakes: Vec::new(),
        }
    }
}

impl CameraEffector {
    pub fn add_kick(&mut self, vertical: (f32, f32), horizontal: (f32, f32), recovery_speed: f32) {
        self.pitch_vel += vertical.0 + fastrand::f32() * (vertical.1 - vertical.0);
        self.yaw_vel += horizontal.0 + fastrand::f32() * (horizontal.1 - horizontal.0);
        self.recovery_speed = recovery_speed;
    }

    #[cfg(feature = "client")]
    pub fn add_shake(&mut self, shake: CameraShake) {
        if shake.duration <= 0.0
            || shake.frequency <= 0.0
            || (shake.translation == Vec3::ZERO
                && shake.rotation == Vec2::ZERO
                && shake.roll == 0.0)
        {
            return;
        }
        self.active_shakes.push(ActiveCameraShake {
            shake,
            age: 0.0,
            seed: fastrand::i32(..),
        });
    }

    pub fn current_zoom_factor(&self) -> f32 {
        let base = (self.base_fov.to_radians() * 0.5).tan();
        let current = (self.current_fov.to_radians() * 0.5).tan();
        if current > 0.0 {
            (base / current).max(1.0)
        } else {
            1.0
        }
    }

    pub fn reset_zoom(&mut self) {
        self.zoom_multiplier = 1.0;
        self.current_fov = self.base_fov;
    }

    #[cfg(feature = "client")]
    pub(crate) fn sample_shakes(&mut self, dt: f32) -> (Vec3, Vec2, f32) {
        let mut translation = Vec3::ZERO;
        let mut rotation = Vec2::ZERO;
        let mut roll = 0.0;
        self.active_shakes.retain_mut(|active| {
            active.age += dt;
            let life = (active.age / active.shake.duration).clamp(0.0, 1.0);
            let envelope = (1.0 - life) * (1.0 - life);
            if envelope <= 0.0 {
                return false;
            }
            let sample_t = active.age * active.shake.frequency;
            translation.x +=
                active.shake.translation.x * envelope * perlin_1d(sample_t, active.seed, 11.0);
            translation.y +=
                active.shake.translation.y * envelope * perlin_1d(sample_t, active.seed, 23.0);
            translation.z +=
                active.shake.translation.z * envelope * perlin_1d(sample_t, active.seed, 37.0);
            rotation.x +=
                active.shake.rotation.x * envelope * perlin_1d(sample_t, active.seed, 41.0);
            rotation.y +=
                active.shake.rotation.y * envelope * perlin_1d(sample_t, active.seed, 53.0);
            roll += active.shake.roll * envelope * perlin_1d(sample_t, active.seed, 67.0);
            true
        });
        (translation, rotation, roll)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CameraShake {
    pub translation: Vec3,
    pub rotation: Vec2,
    pub roll: f32,
    pub duration: f32,
    pub frequency: f32,
}

impl CameraShake {
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            translation: self.translation * scale,
            rotation: self.rotation * scale,
            roll: self.roll * scale,
            ..self
        }
    }
}

#[cfg(feature = "client")]
pub(crate) struct ActiveCameraShake {
    shake: CameraShake,
    age: f32,
    seed: i32,
}

#[cfg(feature = "client")]
fn perlin_1d(x: f32, seed: i32, channel: f32) -> f32 {
    Perlin.seed(seed).sample2([x, channel]) as f32
}
