use bevy::math::{Quat, Vec3};
use wincode_derive::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone, Copy)]
pub struct MyVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone, Copy)]
pub struct MyQuat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}
impl From<Vec3> for MyVec3 {
    fn from(v: Vec3) -> Self {
        Self {
            x: v.x,
            y: v.y,
            z: v.z,
        }
    }
}
impl From<MyVec3> for Vec3 {
    fn from(v: MyVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}
impl From<Quat> for MyQuat {
    fn from(q: Quat) -> Self {
        Self {
            x: q.x,
            y: q.y,
            z: q.z,
            w: q.w,
        }
    }
}
impl From<MyQuat> for Quat {
    fn from(q: MyQuat) -> Self {
        Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }
}
