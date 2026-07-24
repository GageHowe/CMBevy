//! This lets us author levels with human-readable names instead of bevy names. Might also have more use cases in the future

use bevy::prelude::*;

#[rustfmt::skip]
const KEY_EXPANSIONS: &[(&str, &str)] = &[
    ("Transform", "bevy_transform::components::transform::Transform"),
    ("ChildOf", "bevy_ecs::hierarchy::ChildOf"),
    ("Shape", "physics::collider_shape::AuthoredColliderShape"),
    ("MapMeta", "gameplay::level::MapMeta"),
    ("StaticCollider", "gameplay::level::StaticCollider"),
    ("ColliderMaterial", "gameplay::level::ColliderMaterial"),
    ("SceneModel", "gameplay::level::SceneModel"),
    ("CollisionFxMaterial", "gameplay::collision::CollisionFxMaterial"),
    ("SpawnPoint", "gameplay::level::SpawnPoint"),
    ("ScriptTags", "gameplay::level::ScriptTags"),
    ("ScriptZone", "gameplay::level::ScriptZone"),
    ("ZoneEffect", "gameplay::zone_effects::ZoneEffect"),
    ("Spawner", "gameplay::level::Spawner"),
    ("SceneRigidBody", "physics::physics_world::SceneRigidBody"),
    ("InitialVelocity", "physics::physics_world::InitialVelocity"),
    ("InitialAngularVelocity", "physics::physics_world::InitialAngularVelocity"),
    ("CascadeShadowConfig", "bevy_light::cascade::CascadeShadowConfig"),
    ("DirectionalLight", "bevy_light::directional_light::DirectionalLight"),
    ("AreaReverbComponent", "gameplay::components::atmosphere::AreaReverbComponent"),
    ("GravitySource", "gameplay::components::gravity::GravitySource"),
    ("SnapSource", "gameplay::components::snap::SnapSource"),
];

pub fn preprocess_level_text(text: &str) -> Result<String, String> {
    // TODO: Replace this naive string substitution with a real scene-authoring preprocess step.
    // It is easy to break accidentally if a quoted string happens to match one of these keys.
    let mut out = text.to_owned();
    for (key, value) in KEY_EXPANSIONS {
        let from = format!("\"{key}\"");
        let to = format!("\"{value}\"");
        out = out.replace(&from, &to);
    }
    out = out.replace("shadows_enabled:", "shadow_maps_enabled:");
    out = expand_from_euler(&out)?;
    #[cfg(not(feature = "client"))]
    {
        out = strip_render_components(&out);
    }
    Ok(out)
}

pub fn preprocess_level_bytes(raw: &[u8]) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(raw).map_err(|e| format!("level utf8: {e}"))?;
    Ok(preprocess_level_text(text)?.into_bytes())
}

fn expand_from_euler(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;

    while let Some(found) = text[cursor..].find("from_euler(") {
        let start = cursor + found;
        out.push_str(&text[cursor..start]);

        let args_start = start + "from_euler".len();
        let args_end = matching_paren(text, args_start)
            .ok_or_else(|| "from_euler: missing closing ')'".to_string())?;
        let quat = parse_euler_call(&text[args_start..=args_end])?;
        out.push_str(&format!(
            "({:.8}, {:.8}, {:.8}, {:.8})",
            quat.x, quat.y, quat.z, quat.w
        ));
        cursor = args_end + 1;
    }

    out.push_str(&text[cursor..]);
    Ok(out)
}

fn matching_paren(text: &str, open_index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(open_index) != Some(&b'(') {
        return None;
    }

    let mut depth = 0i32;
    for (index, byte) in bytes.iter().enumerate().skip(open_index) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_euler_call(text: &str) -> Result<Quat, String> {
    let tuple = text
        .strip_prefix('(')
        .and_then(|text| text.strip_suffix(')'))
        .ok_or_else(|| "from_euler: invalid call syntax".to_string())?
        .trim();
    let (x, y, z): (f32, f32, f32) = ron::from_str(tuple)
        .map_err(|e| format!("from_euler: expected from_euler((x, y, z)): {e}"))?;

    Ok(Quat::from_euler(
        EulerRot::XYZ,
        x.to_radians(),
        y.to_radians(),
        z.to_radians(),
    ))
}

#[cfg(not(feature = "client"))]
const RENDER_COMPONENT_PREFIXES: &[&str] = &[
    "bevy_light::",
    "bevy_pbr::",
    "bevy_render::",
    "bevy_core_pipeline::",
];

#[cfg(not(feature = "client"))]
fn strip_render_components(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    let mut depth = 0i32;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if !skipping
            && RENDER_COMPONENT_PREFIXES.iter().any(|prefix| {
                trimmed.starts_with('"') && trimmed.contains(prefix) && trimmed.contains("\":")
            })
        {
            skipping = true;
            depth = paren_delta(line);
            if depth <= 0 {
                skipping = false;
            }
            continue;
        }
        if skipping {
            depth += paren_delta(line);
            if depth <= 0 {
                skipping = false;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(not(feature = "client"))]
fn paren_delta(line: &str) -> i32 {
    line.bytes().fold(0, |acc, byte| match byte {
        b'(' => acc + 1,
        b')' => acc - 1,
        _ => acc,
    })
}

#[cfg(test)]
mod tests {
    use super::preprocess_level_text;

    #[test]
    fn expands_from_euler_degrees() {
        let text = r#"
(
  entities: {
    1: (
      components: {
        "Transform": (
          translation: (0.0, 0.0, 0.0),
          rotation: from_euler((0.0, 90.0, 0.0)),
          scale: (1.0, 1.0, 1.0),
        ),
      },
    ),
  },
)
"#;
        let out = preprocess_level_text(text).unwrap();
        assert!(out.contains("rotation: (0.00000000, 0.70710677, 0.00000000, 0.70710677)"));
    }

    #[test]
    fn rejects_wrong_from_euler_arity() {
        let err = preprocess_level_text("rotation: from_euler((0.0, 1.0))").unwrap_err();
        assert!(err.contains("expected from_euler((x, y, z))"));
    }

    #[test]
    fn migrates_bevy_019_directional_light_shadow_field() {
        let out = preprocess_level_text("\"DirectionalLight\": (shadows_enabled: true)").unwrap();
        assert!(out.contains("\"bevy_light::directional_light::DirectionalLight\""));
        assert!(out.contains("shadow_maps_enabled: true"));
        assert!(!out.contains("shadows_enabled"));
    }
}
