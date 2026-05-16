use bevy::prelude::*;
use physics::{collider_flags::ColliderFlags, physics_world::PhysicsWorld};
use rapier3d::prelude::{
    Collider, ColliderHandle, Group, InteractionGroups, InteractionTestMode, RigidBodyHandle,
};

#[derive(Component, Clone, Copy)]
pub struct Shield {
    pub current: f32,
    pub max: f32,
    pub regen_per_sec: f32,
    pub regen_delay_secs: f32,
    pub regen_delay_remaining_secs: f32,
    pub enabled: bool,
    pub collider: Option<ColliderHandle>,
    #[cfg(feature = "client")]
    pub visual: Option<Entity>,
}

impl Shield {
    pub fn new(max: f32, regen_per_sec: f32, regen_delay_secs: f32) -> Self {
        Self {
            current: max,
            max,
            regen_per_sec,
            regen_delay_secs,
            regen_delay_remaining_secs: 0.0,
            enabled: true,
            collider: None,
            #[cfg(feature = "client")]
            visual: None,
        }
    }

    pub fn apply_damage(&mut self, damage: f32) -> bool {
        if !self.enabled || damage <= 0.0 {
            return false;
        }
        self.regen_delay_remaining_secs = self.regen_delay_secs;
        self.current = (self.current - damage).max(0.0);
        if self.current <= 0.0 {
            self.enabled = false;
        }
        true
    }
}

pub struct ShieldPlugin;
impl Plugin for ShieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            regenerate_shields.in_set(crate::AuthoritySystems),
        );
        app.add_systems(FixedUpdate, sync_shield_colliders);
        #[cfg(feature = "client")]
        app.add_systems(FixedUpdate, sync_shield_visuals);
    }
}

pub fn shield_user_data() -> u128 {
    ColliderFlags::SHIELD.bits()
}

pub fn attach_shield_collider(
    body_handle: RigidBodyHandle,
    mut collider: Collider,
    max_health: f32,
    regen_per_sec: f32,
    regen_delay_secs: f32,
    world: &mut PhysicsWorld,
) -> Shield {
    let no_contacts = InteractionGroups::new(Group::ALL, Group::NONE, InteractionTestMode::And);
    collider.set_collision_groups(no_contacts);
    collider.set_solver_groups(no_contacts);
    collider.user_data = shield_user_data();
    let handle = world.collider_set.insert_with_parent(
        collider,
        body_handle,
        &mut world.rigid_body_set,
    );
    let mut shield = Shield::new(max_health, regen_per_sec, regen_delay_secs);
    shield.collider = Some(handle);
    shield
}

fn regenerate_shields(time: Res<Time<Fixed>>, mut shields: Query<&mut Shield>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut shield in &mut shields {
        shield.regen_delay_remaining_secs = (shield.regen_delay_remaining_secs - dt).max(0.0);
        if shield.current >= shield.max || shield.regen_per_sec <= 0.0 {
            continue;
        }
        if shield.regen_delay_remaining_secs > 0.0 {
            continue;
        }
        shield.current = (shield.current + shield.regen_per_sec * dt).min(shield.max);
        if shield.current > 0.0 {
            shield.enabled = true;
        }
    }
}

pub fn sync_shield_colliders(
    world: ResMut<PhysicsWorld>,
    shields: Query<&Shield, Changed<Shield>>,
) {
    let mut world = world;
    for shield in shields.iter() {
        let Some(handle) = shield.collider else {
            continue;
        };
        let Some(collider) = world.collider_set.get_mut(handle) else {
            continue;
        };
        collider.set_enabled(shield.enabled);
    }
}

#[cfg(feature = "client")]
fn sync_shield_visuals(
    mut visibility_q: Query<&mut Visibility>,
    shields: Query<&Shield, Changed<Shield>>,
) {
    for shield in shields.iter() {
        let Some(visual) = shield.visual else {
            continue;
        };
        let Ok(mut visibility) = visibility_q.get_mut(visual) else {
            continue;
        };
        *visibility = if shield.enabled {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// example visuals, a simple box
#[cfg(feature = "client")]
pub fn spawn_box_shield_visual(world: &mut World, parent: Entity, half_extents: Vec3) -> Entity {
    let mesh = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        meshes.add(Cuboid::from_size(half_extents * 2.0))
    };
    let material = {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        materials.add(StandardMaterial {
            base_color: Color::srgba(0.2, 0.7, 1.0, 0.14),
            emissive: LinearRgba::rgb(0.08, 0.18, 0.28),
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            unlit: true,
            ..default()
        })
    };
    let visual = world
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    world.entity_mut(parent).add_child(visual);
    visual
}
