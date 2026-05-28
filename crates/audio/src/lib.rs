#[cfg(feature = "client")]
use std::ffi::CStr;

use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::transform::TransformSystems;
#[cfg(feature = "client")]
use common::config;
#[cfg(feature = "client")]
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, rb_vel};

/* sound names: hardcoded names for sounds used in multiple places */
pub const UI_CLICK_EVENT: &str = "event:/UI/Click";
pub const UI_BACK_EVENT: &str = "event:/UI/Back";

#[cfg(feature = "client")]
#[derive(Resource, Default)]
pub struct AudioOutputDevices {
    pub names: Vec<String>,
    pub default_name: String,
    pub current_name: String,
}

#[cfg(feature = "client")]
#[derive(Resource, Clone)]
pub struct AudioSettings {
    pub output_device: String,
    pub buffer_size: u32,
}

#[cfg(feature = "client")]
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            output_device: String::new(),
            buffer_size: 256,
        }
    }
}

#[derive(Clone, Copy)]
pub struct SoundRequest {
    pub event: &'static str,
    pub position: Option<Vec3>,
    pub velocity: Vec3,
    pub gain: f32,
}

#[derive(Resource, Default)]
pub struct SoundQueue(pub Vec<SoundRequest>);

impl SoundQueue {
    /// for UI sounds, self-fired gunshots, etc
    pub fn play_2d(&mut self, event: &'static str) {
        self.0.push(SoundRequest {
            event,
            position: None,
            velocity: Vec3::ZERO,
            gain: 1.0,
        });
    }

    /// for spatialized sounds
    pub fn play_3d(&mut self, event: &'static str, position: Vec3, velocity: Vec3) {
        // self.play_3d_with_gain(event, position, velocity, 1.0);
        self.0.push(SoundRequest {
            event,
            position: Some(position),
            velocity,
            gain: 1.0,
        });
    }
}

#[derive(Component)]
pub struct SoundEmitter {
    pub event: &'static str,
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
pub struct AudioListener {
    pub position: Vec3,
    pub velocity: Vec3,
    pub rotation: Quat,
    pub active: bool,
}

pub fn queue_ui_sound(queue: &mut SoundQueue, event: &'static str) {
    queue.play_2d(event);
}

#[cfg(feature = "client")]
#[derive(Resource)]
pub struct FmodStudio {
    pub system: fmod::Studio,
    _banks: Vec<fmod::Bank>,
}

#[cfg(feature = "client")]
pub struct SoundPlugin;

#[cfg(feature = "client")]
impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundQueue>()
            .init_resource::<AudioOutputDevices>()
            .init_resource::<AudioSettings>()
            .init_resource::<AudioListener>()
            .add_systems(
                PostStartup,
                (init_fmod, refresh_output_devices, apply_output_device).chain(),
            )
            .add_systems(
                PostUpdate,
                (spawn_instances, update_instances)
                    .chain()
                    .after(TransformSystems::Propagate),
            )
            .add_systems(Last, (sync_listener, flush_queue, update_fmod).chain())
            .add_systems(
                Update,
                apply_output_device.run_if(resource_changed::<AudioSettings>),
            );
    }
}

#[cfg(feature = "client")]
#[derive(Component)]
struct FmodInstance(fmod::EventInstance);

#[cfg(feature = "client")]
impl Drop for FmodInstance {
    fn drop(&mut self) {
        let _ = self.0.stop(fmod::StopMode::AllowFadeout);
        let _ = self.0.release();
    }
}

#[cfg(feature = "client")]
fn init_fmod(mut commands: Commands, settings: Res<AudioSettings>) {
    let buffer_size = settings.buffer_size.clamp(128, 4096);
    let Ok(system) = fmod::Studio::create().inspect_err(|e| warn!("FMOD: create failed: {e:?}"))
    else {
        return;
    };
    let Ok(core) = system.get_core_system() else {
        return;
    };
    if let Err(e) = core.set_dsp_buffer_size(buffer_size, 4) {
        warn!("FMOD: set DSP buffer size to {buffer_size} failed: {e:?}");
    }
    if let Err(e) = core.set_3d_settings(0.5, 1.0, 1.0) {
        warn!("FMOD: set 3D settings failed: {e:?}");
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

#[cfg(feature = "client")]
fn refresh_output_devices(fmod: Option<Res<FmodStudio>>, mut devices: ResMut<AudioOutputDevices>) {
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

#[cfg(feature = "client")]
fn apply_output_device(
    settings: Res<AudioSettings>,
    fmod: Option<Res<FmodStudio>>,
    devices: ResMut<AudioOutputDevices>,
) {
    let Some(fmod) = fmod else { return };
    let target_name = if settings.output_device.is_empty() {
        devices.default_name.clone()
    } else {
        settings.output_device.clone()
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

#[cfg(feature = "client")]
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

#[cfg(feature = "client")]
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

#[cfg(feature = "client")]
fn sync_listener(fmod: Option<Res<FmodStudio>>, listener: Res<AudioListener>) {
    if !listener.active {
        return;
    }
    let Some(fmod) = fmod else { return };
    let _ = fmod.system.set_listener_attributes(
        0,
        attrs(listener.position, listener.velocity, listener.rotation),
        None,
    );
}

#[cfg(feature = "client")]
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
        let _ = instance.set_volume(req.gain);
        let _ = instance.start();
        disable_volume_ramp(&instance);
        let _ = instance.release();
    }
}

#[cfg(feature = "client")]
fn update_fmod(fmod: Option<Res<FmodStudio>>) {
    if let Some(fmod) = fmod {
        let _ = fmod.system.update();
    }
}

#[cfg(feature = "client")]
pub fn set_global_parameter(fmod: &FmodStudio, name: &str, value: f32) {
    let _ = fmod
        .system
        .set_parameter_by_name(name, value, true)
        .inspect_err(|e| warn!("FMOD: set {name}={value:.2} failed: {e:?}"));
}

#[cfg(feature = "client")]
fn create_instance(fmod: &FmodStudio, event: &'static str) -> Option<fmod::EventInstance> {
    fmod.system
        .get_event(event)
        .inspect_err(|e| warn!("FMOD: event '{event}' not found: {e:?}"))
        .ok()?
        .create_instance()
        .ok()
}

#[cfg(feature = "client")]
fn disable_volume_ramp(instance: &fmod::EventInstance) {
    let _ = instance
        .get_channel_group()
        .and_then(|group| group.set_volume_ramp(false));
}

#[cfg(feature = "client")]
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
    Ok(unsafe { CStr::from_ptr(name.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

#[cfg(feature = "client")]
#[inline]
fn attrs(pos: Vec3, vel: Vec3, rot: Quat) -> fmod::Attributes3d {
    fmod::Attributes3d {
        position: v(pos),
        velocity: v(vel),
        forward: v(rot * Vec3::NEG_Z),
        up: v(rot * Vec3::Y),
    }
}

#[cfg(feature = "client")]
#[inline]
fn v(vec: Vec3) -> fmod::Vector {
    fmod::Vector {
        x: vec.x,
        y: vec.y,
        z: vec.z,
    }
}
