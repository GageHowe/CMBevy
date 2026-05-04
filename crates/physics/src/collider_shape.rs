use bevy::prelude::*;
use rapier3d::prelude::*;

#[derive(Clone, Reflect)]
#[reflect(Default)]
pub enum AuthoredColliderShape {
    Ball(f32),
    Cuboid(Vec3),
    Capsule {
        half_height: f32,
        radius: f32,
    },
    /// Local asset path or `sha256:...` remote ref to an OBJ file containing VHACD convex hulls.
    ConvexHulls(String),
}

impl Default for AuthoredColliderShape {
    fn default() -> Self {
        Self::Ball(1.0)
    }
}

impl AuthoredColliderShape {
    pub fn build_primitive_collider(&self, scale: f32) -> Option<Collider> {
        match self {
            Self::Cuboid(he) => {
                Some(ColliderBuilder::cuboid(he.x * scale, he.y * scale, he.z * scale).build())
            }
            Self::Ball(radius) => Some(ColliderBuilder::ball(radius * scale).build()),
            Self::Capsule { half_height, radius } => {
                Some(ColliderBuilder::capsule_y(half_height * scale, radius * scale).build())
            }
            Self::ConvexHulls(_) => None,
        }
    }

    pub fn contains_point(&self, scale: f32, position: Vec3, rotation: Quat, point: Vec3) -> bool {
        let local = rotation.inverse() * (point - position);
        match self {
            Self::Ball(radius) => local.length_squared() <= (radius * scale).powi(2),
            Self::Cuboid(half_extents) => {
                let he = *half_extents * scale;
                local.x.abs() <= he.x && local.y.abs() <= he.y && local.z.abs() <= he.z
            }
            Self::Capsule { half_height, radius } => {
                let half_height = half_height * scale;
                let radius = radius * scale;
                let clamped_y = local.y.clamp(-half_height, half_height);
                let nearest = Vec3::new(0.0, clamped_y, 0.0);
                local.distance_squared(nearest) <= radius.powi(2)
            }
            Self::ConvexHulls(_) => false,
        }
    }
}
