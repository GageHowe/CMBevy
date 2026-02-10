use bevy::math::{Quat, Vec3};
use wincode_derive::{SchemaRead, SchemaWrite};

// MATH WRAPPERS
// Critical Mass uses its own types to allow serialization and implementation of more functions/traits. Use these when possible.

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone, Copy)]
pub struct CMVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone, Copy)]
pub struct CMQuat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

// convert bevy types to and from Critical Mass types
impl From<Vec3> for CMVec3 {
    fn from(v: Vec3) -> Self {
        Self {
            x: v.x,
            y: v.y,
            z: v.z,
        }
    }
}
impl From<CMVec3> for Vec3 {
    fn from(v: CMVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}
impl From<Quat> for CMQuat {
    fn from(q: Quat) -> Self {
        Self {
            x: q.x,
            y: q.y,
            z: q.z,
            w: q.w,
        }
    }
}
impl From<CMQuat> for Quat {
    fn from(q: CMQuat) -> Self {
        Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }
}
