//! This lets us author levels with human-readable names instead of bevy names. Might also have more use cases in the future

#[rustfmt::skip]
const KEY_EXPANSIONS: &[(&str, &str)] = &[
    ("Transform", "bevy_transform::components::transform::Transform"),
    ("ChildOf", "bevy_ecs::hierarchy::ChildOf"),
    ("Shape", "physics::collider_shape::AuthoredColliderShape"),
    ("MapMeta", "game_objects::level::MapMeta"),
    ("StaticCollider", "game_objects::level::StaticCollider"),
    ("SceneModel", "game_objects::level::SceneModel"),
    ("CollisionFxMaterial", "game_objects::collision::CollisionFxMaterial"),
    ("SpawnPoint", "game_objects::level::SpawnPoint"),
    ("ScriptTags", "game_objects::level::ScriptTags"),
    ("ScriptZone", "game_objects::level::ScriptZone"),
    ("ZoneEffect", "game_objects::zone_effects::ZoneEffect"),
    ("Spawner", "game_objects::level::Spawner"),
    ("SceneRigidBody", "physics::physics_world::SceneRigidBody"),
    ("InitialVelocity", "physics::physics_world::InitialVelocity"),
    ("InitialAngularVelocity", "physics::physics_world::InitialAngularVelocity"),
    ("CascadeShadowConfig", "bevy_light::cascade::CascadeShadowConfig"),
    ("DirectionalLight", "bevy_light::directional_light::DirectionalLight"),
    ("AreaReverbComponent", "game_objects::components::atmosphere::AreaReverbComponent"),
    ("GravitySource", "game_objects::components::gravity::GravitySource"),
    ("SnapSource", "game_objects::components::snap::SnapSource"),
];

pub fn preprocess_level_text(text: &str) -> String {
    // TODO: Replace this naive string substitution with a real scene-authoring preprocess step.
    // It is easy to break accidentally if a quoted string happens to match one of these keys.
    let mut out = text.to_owned();
    for (key, value) in KEY_EXPANSIONS {
        let from = format!("\"{key}\"");
        let to = format!("\"{value}\"");
        out = out.replace(&from, &to);
    }
    out
}

pub fn preprocess_level_bytes(raw: &[u8]) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(raw).map_err(|e| format!("level utf8: {e}"))?;
    Ok(preprocess_level_text(text).into_bytes())
}
