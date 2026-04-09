//! Indexes map-authored script tags so Lua can resolve important entities quickly.

use bevy::prelude::*;
use game_objects::level::ScriptTags;
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct ScriptTagIndex {
    by_tag: HashMap<String, Vec<Entity>>,
    by_entity: HashMap<Entity, Vec<String>>,
}

impl ScriptTagIndex {
    pub fn get(&self, tag: &str) -> &[Entity] {
        self.by_tag.get(tag).map_or(&[], Vec::as_slice)
    }

    pub fn has(&self, entity: Entity, tag: &str) -> bool {
        self.by_entity
            .get(&entity)
            .is_some_and(|tags| tags.iter().any(|value| value == tag))
    }

    fn remove_entity(&mut self, entity: Entity) {
        let Some(tags) = self.by_entity.remove(&entity) else {
            return;
        };
        for tag in tags {
            let Some(entities) = self.by_tag.get_mut(&tag) else {
                continue;
            };
            entities.retain(|value| *value != entity);
            if entities.is_empty() {
                self.by_tag.remove(&tag);
            }
        }
    }

    fn insert_entity(&mut self, entity: Entity, tags: &[String]) {
        self.remove_entity(entity);
        let mut unique = Vec::new();
        for tag in tags {
            if tag.is_empty() || unique.iter().any(|value| value == tag) {
                continue;
            }
            unique.push(tag.clone());
            self.by_tag.entry(tag.clone()).or_default().push(entity);
        }
        if !unique.is_empty() {
            self.by_entity.insert(entity, unique);
        }
    }
}

pub(crate) fn sync_script_tags(
    mut index: ResMut<ScriptTagIndex>,
    tagged: Query<(Entity, &ScriptTags), Or<(Added<ScriptTags>, Changed<ScriptTags>)>>,
    mut removed: RemovedComponents<ScriptTags>,
) {
    for entity in removed.read() {
        index.remove_entity(entity);
    }
    for (entity, tags) in &tagged {
        index.insert_entity(entity, &tags.tags);
    }
}
