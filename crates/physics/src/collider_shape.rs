use bevy::prelude::*;
use rapier3d::prelude::*;

#[derive(Clone, Reflect)]
#[reflect(Default)]
pub enum ColliderShape {
    Ball(f32),
    Cuboid(Vec3),
    Capsule {
        half_height: f32,
        radius: f32,
    },
    /// Local asset path or `sha256:...` remote ref to an OBJ file containing VHACD convex hulls.
    ConvexHulls(String),
}

impl Default for ColliderShape {
    fn default() -> Self {
        Self::Ball(1.0)
    }
}

impl ColliderShape {
    pub fn build_primitive_collider(&self, scale: f32) -> Option<Collider> {
        match self {
            Self::Cuboid(he) => {
                Some(ColliderBuilder::cuboid(he.x * scale, he.y * scale, he.z * scale).build())
            }
            Self::Ball(radius) => Some(ColliderBuilder::ball(radius * scale).build()),
            Self::Capsule {
                half_height,
                radius,
            } => Some(ColliderBuilder::capsule_y(half_height * scale, radius * scale).build()),
            Self::ConvexHulls(_) => None,
        }
    }
}
