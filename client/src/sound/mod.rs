use bevy::prelude::*;
use game_objects::sound::SoundQueue;

pub struct SoundPlugin;
impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundQueue>();
        fmod_impl::build(app);
    }
}

mod fmod_impl {
    use bevy::prelude::*;
    use bevy::transform::TransformSystems;
    use game_objects::sound::{SoundEmitter, SoundQueue};
    use game_objects::atmosphere::AtmosphereComponent;
    use game_objects::pawn::Possessed;
    use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_vel};

    /// FMOD Studio .bank files to load at startup, relative to the working directory.
    const BANK_PATHS: &[&str] = &[
        "assets/fmod-out/Desktop/Master.bank",
        "assets/fmod-out/Desktop/Master.strings.bank",
        "assets/fmod-out/Desktop/SFX.bank",
    ];
    const GLOBAL_ATMOSPHERE_REVERB: &str = "GlobalAtmosphereReverb";
    const IN_SPACE_FILTER: &str = "InSpaceFilter";

    #[derive(Resource)]
    pub struct FmodStudio {
        pub system: fmod::Studio,
        /// Keep bank handles alive — releasing a bank unloads all its events.
        _banks: Vec<fmod::Bank>,
    }

    /// Live FMOD instance stored as a component on the emitting entity.
    /// Stops and releases on Drop — handles cleanup for both despawn and component removal.
    #[derive(Component)]
    struct FmodInstance(fmod::EventInstance);
    impl Drop for FmodInstance {
        fn drop(&mut self) {
            let _ = self.0.stop(fmod::StopMode::AllowFadeout);
            let _ = self.0.release();
        }
    }

    pub fn build(app: &mut App) {
        app.add_systems(Startup, init_fmod)
            .add_systems(PostUpdate, (
                spawn_instances,
                update_instances,
                flush_queue,
                sync_listener,
                update_atmosphere_reverb,
                update_fmod,
            ).chain().after(TransformSystems::Propagate));
    }

    /// called once at startup before any FMOD use
    /// stable, do not touch.
    fn init_fmod(mut commands: Commands) {
        let Ok(system) = fmod::Studio::create()
            .and_then(|s| {
                s.initialize(
                    512,
                    fmod::StudioInit::NORMAL | fmod::StudioInit::LIVEUPDATE,
                    fmod::Init::RIGHTHANDED_3D,
                    None,
                )?;
                Ok(s)
            })
            .inspect_err(|e| warn!("FMOD: init failed: {e:?}"))
        else { return };
        let mut banks = Vec::new();
        for path in BANK_PATHS {
            match system.load_bank_file(path, fmod::LoadBank::NORMAL) {
                Ok(bank) => banks.push(bank),
                Err(e) => warn!("FMOD: could not load '{path}': {e:?}"),
            }
        }
        commands.insert_resource(FmodStudio { system, _banks: banks });
    }

    /// Creates and starts an FmodInstance for each new SoundEmitter.
    fn spawn_instances(
        mut commands: Commands,
        fmod: Option<Res<FmodStudio>>,
        emitters: Query<(Entity, &SoundEmitter, &GlobalTransform), Added<SoundEmitter>>,
    ) {
        let Some(fmod) = fmod else { return };
        for (entity, emitter, gt) in &emitters {
            let Some(instance) = create_instance(&fmod, emitter.event) else { continue };
            let (_, rot, pos) = gt.to_scale_rotation_translation();
            let _ = instance.set_3d_attributes(attrs(pos, Vec3::ZERO, rot));
            let _ = instance.start();
            commands.entity(entity).insert(FmodInstance(instance));
        }
    }

    /// Syncs 3D position and velocity for all persistent instances each frame.
    fn update_instances(
        world: Res<PhysicsWorld>,
        instances: Query<(&FmodInstance, &GlobalTransform, Option<&RigidBodyHandleComponent>)>,
    ) {
        for (inst, gt, rb) in &instances {
            let (_, rot, pos) = gt.to_scale_rotation_translation();
            let vel = rb
                .and_then(|h| world.rigid_body_set.get(h.0))
                .map(rb_vel)
                .unwrap_or(Vec3::ZERO);
            let _ = inst.0.set_3d_attributes(attrs(pos, vel, rot));
        }
    }

    /// Drains SoundQueue and fires one-shot instances: create → set attrs → start → release.
    /// FMOD destroys the instance once it finishes playing.
    /// position: None = 2D event (no 3D attrs needed); position: Some = 3D spatialized.
    fn flush_queue(fmod: Option<Res<FmodStudio>>, mut queue: ResMut<SoundQueue>) {
        let Some(fmod) = fmod else { queue.0.clear(); return };
        for req in queue.0.drain(..) {
            let Some(instance) = create_instance(&fmod, req.event) else { continue };
            if let Some(pos) = req.position {
                let _ = instance.set_3d_attributes(attrs(pos, req.velocity, Quat::IDENTITY));
            }
            let _ = instance.start();
            let _ = instance.release();
        }
    }

    /// Syncs camera position/orientation and possessed pawn velocity to the FMOD listener.
    fn sync_listener(
        fmod: Option<Res<FmodStudio>>,
        camera: Query<&GlobalTransform, With<Camera3d>>,
        possessed: Query<&RigidBodyHandleComponent, With<Possessed>>,
        world: Res<PhysicsWorld>,
    ) {
        let (Some(fmod), Ok(gt)) = (fmod, camera.single()) else { return };
        let (_, rot, pos) = gt.to_scale_rotation_translation();
        let vel = possessed.single().ok()
            .and_then(|h| world.rigid_body_set.get(h.0))
            .map(|rb| { let v = rb.linvel(); Vec3::new(v.x, v.y, v.z) })
            .unwrap_or(Vec3::ZERO);
        let _ = fmod.system.set_listener_attributes(0, attrs(pos, vel, rot), None);
    }

    fn update_fmod(fmod: Option<Res<FmodStudio>>) {
        if let Some(fmod) = fmod {
            let _ = fmod.system.update();
        }
    }

    /// Drives the global Studio parameter "GlobalAtmosphereReverb" (0–1) based on listener
    /// proximity to the nearest atmosphere reverb zone. Works for all events (2D and 3D)
    /// since it's a global parameter — reverb properties are configured in FMOD Studio.
    fn update_atmosphere_reverb(
        fmod: Option<Res<FmodStudio>>,
        camera: Query<&GlobalTransform, With<Camera3d>>,
        atmospheres: Query<(&AtmosphereComponent, &GlobalTransform)>,
    ) {
        let (Some(fmod), Ok(cam_gt)) = (fmod, camera.single()) else {
            return;
        };
        let listener_pos = cam_gt.translation();

        // take the strongest blend across all reverb zones
        let blend = atmospheres.iter()
            .filter_map(|(atmo, gt)| {
                let rs = atmo.reverb.as_ref()?;
                let dist = listener_pos.distance(gt.translation());
                // 1.0 inside min_distance, fades to 0.0 at max_distance
                Some(1.0 - ((dist - rs.min_distance) / (rs.max_distance - rs.min_distance)).clamp(0.0, 1.0))
            })
            .fold(0.0_f32, f32::max);

        let _ = fmod
            .system
            .set_parameter_by_name(GLOBAL_ATMOSPHERE_REVERB, blend, true)
            .inspect_err(|e| warn!("FMOD: set GlobalAtmosphereReverb={blend:.2} failed: {e:?}"));
        let _ = fmod
            .system
            .set_parameter_by_name(IN_SPACE_FILTER, 1.0 - blend, true)
            .inspect_err(|e| warn!("FMOD: set InSpaceFilter={:.2} failed: {e:?}", 1.0 - blend));
    }

    fn create_instance(fmod: &FmodStudio, event: &'static str) -> Option<fmod::EventInstance> {
        fmod.system.get_event(event)
            .inspect_err(|e| warn!("FMOD: event '{event}' not found: {e:?}")).ok()?
            .create_instance().ok()
    }

    #[inline]
    fn attrs(pos: Vec3, vel: Vec3, rot: Quat) -> fmod::Attributes3d {
        fmod::Attributes3d {
            position: v(pos),
            velocity: v(vel),
            forward: v(rot * Vec3::NEG_Z),
            up: v(rot * Vec3::Y),
        }
    }

    #[inline]
    fn v(vec: Vec3) -> fmod::Vector {
        fmod::Vector { x: vec.x, y: vec.y, z: vec.z }
    }
}
