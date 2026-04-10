use bevy::prelude::*;
use game_objects::sound::SoundQueue;

pub const UI_CLICK_EVENT: &str = "event:/UI/Click";
pub const UI_BACK_EVENT: &str = "event:/UI/Back";

#[derive(Resource, Default)]
pub struct AudioOutputDevices {
    pub names: Vec<String>,
    pub default_name: String,
    pub current_name: String,
}

pub fn queue_ui_sound(queue: &mut SoundQueue, event: &'static str) {
    queue.play_2d(event);
}

pub struct SoundPlugin;
impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundQueue>()
            .init_resource::<AudioOutputDevices>();
        fmod_impl::build(app);
    }
}

mod fmod_impl {
    use super::AudioOutputDevices;
    use crate::settings::Settings;
    use bevy::prelude::*;
    use bevy::transform::TransformSystems;
    use common::config;
    use game_objects::components::atmosphere::AreaReverbComponent;
    use game_objects::pawn::Possessed;
    use game_objects::sound::{SoundEmitter, SoundQueue};
    use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_vel};
    use std::ffi::CStr;

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
        app.add_systems(
            PostStartup,
            (init_fmod, refresh_output_devices, apply_output_device).chain(),
        )
        .add_systems(
            PostUpdate,
            (
                spawn_instances,
                update_instances,
                sync_listener,
                update_atmosphere_reverb,
            )
                .chain()
                .after(TransformSystems::Propagate),
        )
        .add_systems(Last, (flush_queue, update_fmod).chain())
        .add_systems(
            Update,
            apply_output_device.run_if(resource_changed::<Settings>),
        );
    }

    /// called once at startup before any FMOD use
    /// stable, do not touch.
    fn init_fmod(mut commands: Commands, settings: Option<Res<Settings>>) {
        let buffer_size = settings
            .as_ref()
            .map_or(256, |settings| settings.fmod_buffer_size.clamp(128, 4096));
        let Ok(system) =
            fmod::Studio::create().inspect_err(|e| warn!("FMOD: create failed: {e:?}"))
        else {
            return;
        };
        let Ok(core) = system.get_core_system() else {
            return;
        };
        if let Err(e) = core.set_dsp_buffer_size(buffer_size, 4) {
            warn!("FMOD: set DSP buffer size to {buffer_size} failed: {e:?}");
        }
        let Ok(()) = system
            .initialize(
                512,
                fmod::StudioInit::NORMAL | fmod::StudioInit::LIVEUPDATE,
                fmod::Init::RIGHTHANDED_3D,
                None,
            )
            .inspect_err(|e| warn!("FMOD: init failed: {e:?}"))
        else {
            return;
        };
        let bank_paths = [
            config::asset_dir().join("fmod-out/Desktop/Master.bank"),
            config::asset_dir().join("fmod-out/Desktop/Master.strings.bank"),
            config::asset_dir().join("fmod-out/Desktop/SFX.bank"),
            config::asset_dir().join("fmod-out/Desktop/UI.bank"),
        ];
        let mut banks = Vec::new();
        for path in &bank_paths {
            match system.load_bank_file(path.to_string_lossy().as_ref(), fmod::LoadBank::NORMAL) {
                Ok(bank) => banks.push(bank),
                Err(e) => warn!("FMOD: could not load '{}': {e:?}", path.display()),
            }
        }
        commands.insert_resource(FmodStudio {
            system,
            _banks: banks,
        });
    }

    fn refresh_output_devices(
        fmod: Option<Res<FmodStudio>>,
        mut devices: ResMut<AudioOutputDevices>,
    ) {
        let Some(fmod) = fmod else { return };
        let Ok(core) = fmod.system.get_core_system() else {
            return;
        };
        let Ok(driver_count) = core.get_num_drivers() else {
            return;
        };
        devices.names.clear();
        for id in 0..driver_count {
            match driver_name(&core, id) {
                Ok(name) => devices.names.push(name),
                Err(e) => warn!("FMOD: could not query driver {id}: {e:?}"),
            }
        }
        if let Ok(current) = core.get_driver()
            && let Ok(name) = driver_name(&core, current)
        {
            if devices.default_name.is_empty() {
                devices.default_name = name.clone();
            }
            devices.current_name = name;
        }
    }

    fn apply_output_device(
        settings: Option<Res<Settings>>,
        fmod: Option<Res<FmodStudio>>,
        devices: ResMut<AudioOutputDevices>,
    ) {
        let (Some(settings), Some(fmod)) = (settings, fmod) else {
            return;
        };
        let target_name = if settings.audio_output_device.is_empty() {
            devices.default_name.clone()
        } else {
            settings.audio_output_device.clone()
        };
        if target_name.is_empty() || devices.current_name == target_name {
            return;
        }
        let Some(index) = devices.names.iter().position(|name| *name == target_name) else {
            warn!("FMOD: output device '{target_name}' not found");
            return;
        };
        let Ok(core) = fmod.system.get_core_system() else {
            return;
        };
        if let Err(e) = core.set_driver(index as i32) {
            warn!("FMOD: failed to switch output device to '{target_name}': {e:?}");
            return;
        }
        refresh_output_devices(Some(fmod), devices);
    }

    /// Creates and starts an FmodInstance for each new SoundEmitter.
    fn spawn_instances(
        mut commands: Commands,
        fmod: Option<Res<FmodStudio>>,
        emitters: Query<(Entity, &SoundEmitter, &GlobalTransform), Added<SoundEmitter>>,
    ) {
        let Some(fmod) = fmod else { return };
        for (entity, emitter, gt) in &emitters {
            let Some(instance) = create_instance(&fmod, emitter.event) else {
                continue;
            };
            let (_, rot, pos) = gt.to_scale_rotation_translation();
            let _ = instance.set_3d_attributes(attrs(pos, Vec3::ZERO, rot));
            let _ = instance.start();
            disable_volume_ramp(&instance);
            commands.entity(entity).insert(FmodInstance(instance));
        }
    }

    /// Syncs 3D position and velocity for all persistent instances each frame.
    fn update_instances(
        world: Res<PhysicsWorld>,
        instances: Query<(
            &FmodInstance,
            &GlobalTransform,
            Option<&RigidBodyHandleComponent>,
        )>,
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
        let Some(fmod) = fmod else {
            queue.0.clear();
            return;
        };
        for req in queue.0.drain(..) {
            let Some(instance) = create_instance(&fmod, req.event) else {
                continue;
            };
            if let Some(pos) = req.position {
                let _ = instance.set_3d_attributes(attrs(pos, req.velocity, Quat::IDENTITY));
            }
            let _ = instance.start();
            disable_volume_ramp(&instance);
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
        let (Some(fmod), Ok(gt)) = (fmod, camera.single()) else {
            return;
        };
        let (_, rot, pos) = gt.to_scale_rotation_translation();
        let vel = possessed
            .single()
            .ok()
            .and_then(|h| world.rigid_body_set.get(h.0))
            .map(|rb| {
                let v = rb.linvel();
                Vec3::new(v.x, v.y, v.z)
            })
            .unwrap_or(Vec3::ZERO);
        let _ = fmod
            .system
            .set_listener_attributes(0, attrs(pos, vel, rot), None);
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
        atmospheres: Query<(&AreaReverbComponent, &GlobalTransform)>,
    ) {
        let (Some(fmod), Ok(cam_gt)) = (fmod, camera.single()) else {
            return;
        };
        let listener_pos = cam_gt.translation();

        // take the strongest blend across all reverb zones
        let blend = atmospheres
            .iter()
            .map(|(reverb, gt)| {
                let dist = listener_pos.distance(gt.translation());
                // 1.0 inside min_distance, fades to 0.0 at max_distance
                1.0 - ((dist - reverb.min_distance) / (reverb.max_distance - reverb.min_distance))
                    .clamp(0.0, 1.0)
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
        fmod.system
            .get_event(event)
            .inspect_err(|e| warn!("FMOD: event '{event}' not found: {e:?}"))
            .ok()?
            .create_instance()
            .ok()
    }

    fn disable_volume_ramp(instance: &fmod::EventInstance) {
        let _ = instance
            .get_channel_group()
            .and_then(|group| group.set_volume_ramp(false));
    }

    fn driver_name(core: &fmod::System, id: i32) -> Result<String, fmod::Error> {
        let mut name = vec![0_i8; 256];
        let mut guid = fmod::ffi::FMOD_GUID::default();
        let mut system_rate = 0_i32;
        let mut speaker_mode = fmod::ffi::FMOD_SPEAKERMODE::default();
        let mut speaker_mode_channels = 0_i32;
        let result = unsafe {
            fmod::ffi::FMOD_System_GetDriverInfo(
                core.as_mut_ptr(),
                id,
                name.as_mut_ptr(),
                name.len() as i32,
                &mut guid,
                &mut system_rate,
                &mut speaker_mode,
                &mut speaker_mode_channels,
            )
        };
        if result != fmod::ffi::FMOD_OK {
            return Err(fmod::Error::Fmod {
                function: "FMOD_System_GetDriverInfo".to_string(),
                code: result,
                message: fmod::errors::map_fmod_error(result).to_string(),
            });
        }
        let name = unsafe { CStr::from_ptr(name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        Ok(name)
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
        fmod::Vector {
            x: vec.x,
            y: vec.y,
            z: vec.z,
        }
    }
}
