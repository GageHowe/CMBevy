use crate::{GameObjectKind, GameObject};
use common::interaction::Interactable;
use physics::physics_world::*;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy_egui::input::EguiWantsInput;
use rapier3d::prelude::*;
use super::*;
use super::vehicle::VehicleComponent;

const THRUST:      f32 = 0.2;
const ROLL_SPEED:  f32 = 1.5;

#[derive(Component, Default, Reflect)]
pub struct SpaceshipPawnComponent;

impl Pawn for SpaceshipPawnComponent {
    fn apply_input(&mut self, world: &mut PhysicsWorld, body: &RigidBodyHandleComponent, input: PawnInputKind) {
        if let PawnInputKind::Spaceship(i) = input { apply_spaceship_movement(world, body, i, self); }
    }
}

impl GameObject for SpaceshipPawnComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform { translation: cmd.position.into(), rotation: cmd.rotation.into(), ..default() };
        world.entity_mut(entity).insert((
            SpaceshipPawnComponent,
            // marks this entity as a driveable vehicle for enter/exit mechanics
            VehicleComponent::default(),
            GameObjectKind::Spaceship,
            Transform::from(transform),
            cmd.net_id.clone(),
            Interactable { range: 4.0 },
        ));
        let rb_handle = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let rb = RigidBodyBuilder::dynamic().translation(transform.translation).build();
            let rb_handle = physics.insert_body(entity, rb);
            let col = ColliderBuilder::cuboid(1.5, 1.0, 3.0).build();
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
            collider_set.insert_with_parent(col, rb_handle, rigid_body_set);
            rb_handle
        };
        world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
        #[cfg(feature = "client")]
        {
            let mesh = world.resource_mut::<Assets<Mesh>>().add(bevy::math::primitives::Cuboid::new(3.0, 2.0, 6.0));
            let material = world.resource_mut::<Assets<StandardMaterial>>().add(Color::srgb(0.2, 0.5, 0.8));
            world.entity_mut(entity).insert((Mesh3d(mesh), MeshMaterial3d(material), Visibility::default()));
        }
    }
}

pub struct SpaceshipPlugin;
impl Plugin for SpaceshipPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedPreUpdate, (
            gather_spaceship_input
                .run_if(resource_exists::<ButtonInput<KeyCode>>)
                .in_set(GatherInputSet),
            move_pawns::<SpaceshipPawnComponent>().in_set(MovePawnsSet),
        ).chain());
    }
}

fn gather_spaceship_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    sensitivity: Res<MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    mut pawns: Query<&mut Possessed, With<SpaceshipPawnComponent>>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) { return; }
    if cursor_q.grab_mode == CursorGrabMode::None { return; }
    let Ok(mut possessed) = pawns.single_mut() else { return };

    let mut input = common::SpaceshipInput::default();
    if keyboard.pressed(KeyCode::KeyW) { input.forward += 1.0; }
    if keyboard.pressed(KeyCode::KeyS) { input.forward -= 1.0; }
    if keyboard.pressed(KeyCode::KeyD) { input.right += 1.0; }
    if keyboard.pressed(KeyCode::KeyA) { input.right -= 1.0; }
    if keyboard.pressed(KeyCode::Space) { input.up += 1.0; }
    if keyboard.pressed(KeyCode::ControlLeft) { input.up -= 1.0; }
    if keyboard.pressed(KeyCode::KeyQ) { input.roll -= 1.0; }
    if keyboard.pressed(KeyCode::KeyE) { input.roll += 1.0; }
    input.ability1 = keyboard.pressed(KeyCode::ShiftLeft);
    let s = sensitivity.0;
    input.yaw   = -mouse.delta.x * s;
    input.pitch = -mouse.delta.y * s;

    possessed.push(PawnInputKind::Spaceship(input));
}

pub fn apply_spaceship_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: common::SpaceshipInput,
    _spaceship: &mut SpaceshipPawnComponent,
) {
    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else { return };
    let rotation = body.rotation();
    let local_right   = rotation * Vector3::new(1.0, 0.0, 0.0);
    let local_up      = rotation * Vector3::new(0.0, 1.0, 0.0);
    let local_forward = rotation * Vector3::new(0.0, 0.0, 1.0);

    let impulse = (local_right * input.right + local_up * input.up + local_forward * input.forward) * THRUST;
    body.apply_impulse(impulse, true);

    let angvel = local_up      * input.yaw   * 60.0
               + local_right   * input.pitch  * 60.0
               + local_forward * input.roll   * ROLL_SPEED;
    body.set_angvel(angvel, true);
}
