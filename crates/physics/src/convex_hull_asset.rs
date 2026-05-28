use std::{fs, path::Path};

use bevy::prelude::*;
use rapier3d::prelude::{Collider, ColliderBuilder, Pose, SharedShape};

/// converts the string contents of a .obj file into a rapier3d Collider
pub fn parse_obj_compound(text: &str, scale: f32) -> Option<Collider> {
    let s = if scale == 0.0 { 1.0 } else { scale };
    let mut shapes: Vec<(Pose, SharedShape)> = Vec::new();
    let mut verts: Vec<Vec3> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("o ") {
            if !verts.is_empty() {
                shapes.push((Pose::IDENTITY, SharedShape::convex_hull(&verts)?));
                verts.clear();
            }
        } else if line.starts_with("v ") {
            let mut p = line[2..].split_whitespace();
            let x: f32 = p.next()?.parse().ok()?;
            let y: f32 = p.next()?.parse().ok()?;
            let z: f32 = p.next()?.parse().ok()?;
            verts.push(Vec3::new(x * s, y * s, z * s));
        }
    }
    if !verts.is_empty() {
        shapes.push((Pose::IDENTITY, SharedShape::convex_hull(&verts)?));
    }

    if shapes.is_empty() {
        return None;
    }
    Some(ColliderBuilder::compound(shapes).build())
}

pub fn load_convex_hull_blocking(path: impl AsRef<Path>, scale: f32) -> Option<Collider> {
    let text = fs::read_to_string(path).ok()?;
    parse_obj_compound(&text, scale)
}
