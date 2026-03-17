use bevy::prelude::*;
use lanyard::Utf8CString;

/// FMOD Studio .bank files to load at startup, relative to the working directory.
const BANK_PATHS: &[&str] = &[
    "assets/fmod/Master.bank",
    "assets/fmod/Master.strings.bank",
    "assets/fmod/Weapons.bank",
    "assets/fmod/Ambience.bank",
];

#[derive(Resource)]
pub struct FmodStudio {
    pub system: fmod::studio::System,
    // Keep bank handles alive — releasing a bank unloads all its events.
    _banks: Vec<fmod::studio::Bank>,
}

pub struct SoundRequest {
    pub event: &'static str,
    pub position: Option<Vec3>,
    pub velocity: Option<Vec3>,
}

/// Systems push requests here; `flush_sound_queue` drains them each frame.
/// Replaces Bevy events for audio (no Bevy events per project rules).
#[derive(Resource, Default)]
pub struct SoundQueue(pub Vec<SoundRequest>);

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundQueue>()
            .add_systems(Startup, init_fmod)
            .add_systems(PostUpdate, (sync_listener, flush_sound_queue, update_fmod).chain());
    }
}

fn init_fmod(mut commands: Commands) {
    // FMOD_INIT_RIGHTHANDED_3D matches Bevy's right-handed Y-up coordinate system,
    // so Vec3 values can be passed directly without axis conversion.
    // Safety: called once at app startup on the main thread before any FMOD use.
    let Ok(system) = (unsafe { fmod::studio::SystemBuilder::new() })
        .and_then(|b| b.build(512, fmod::studio::InitFlags::NORMAL, fmod::InitFlags::RIGHTHANDED_3D))
        .inspect_err(|e| warn!("FMOD: init failed: {e:?}"))
    else { return };

    let mut banks = Vec::new();
    for path in BANK_PATHS {
        let Ok(cpath) = Utf8CString::new(*path)
            .inspect_err(|e| warn!("FMOD: invalid bank path '{path}': {e:?}"))
        else { continue };
        match system.load_bank_file(&cpath, fmod::studio::LoadBankFlags::NORMAL) {
            Ok(bank) => banks.push(bank),
            Err(e) => warn!("FMOD: could not load '{path}': {e:?}"),
        }
    }

    commands.insert_resource(FmodStudio { system, _banks: banks });
}

/// Pushes the camera's world transform to FMOD's listener each frame.
fn sync_listener(
    fmod: Option<Res<FmodStudio>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let (Some(fmod), Ok(gt)) = (fmod, camera.single()) else { return };
    let (_, rot, pos) = gt.to_scale_rotation_translation();
    let _ = fmod.system.set_listener_attributes(
        0,
        fmod::Attributes3D {
            position: v(pos),
            velocity: fmod::Vector::default(),
            forward: v(rot * Vec3::NEG_Z),
            up: v(rot * Vec3::Y),
        },
        None,
    );
}

/// Drains SoundQueue and fires one-shot FMOD Studio event instances.
fn flush_sound_queue(fmod: Option<Res<FmodStudio>>, mut queue: ResMut<SoundQueue>) {
    let Some(fmod) = fmod else { queue.0.clear(); return };
    for req in queue.0.drain(..) {
        let Ok(cpath) = Utf8CString::new(req.event) else { continue };
        let Ok(desc) = fmod.system.get_event(&cpath)
            .inspect_err(|e| warn!("FMOD: event '{}' not found: {e:?}", req.event))
        else { continue };
        let Ok(instance) = desc.create_instance() else { continue };
        if let Some(pos) = req.position {
            let vel = req.velocity.unwrap_or(Vec3::ZERO);
            let _ = instance.set_3d_attributes(fmod::Attributes3D {
                position: v(pos),
                velocity: v(vel),
                forward: fmod::Vector { x: 0.0, y: 0.0, z: -1.0 },
                up: fmod::Vector { x: 0.0, y: 1.0, z: 0.0 },
            });
        }
        let _ = instance.start();
        // Release immediately — FMOD destroys the instance once it finishes playing.
        let _ = instance.release();
    }
}

fn update_fmod(fmod: Option<Res<FmodStudio>>) {
    if let Some(fmod) = fmod {
        let _ = fmod.system.update();
    }
}

#[inline]
fn v(vec: Vec3) -> fmod::Vector {
    fmod::Vector { x: vec.x, y: vec.y, z: vec.z }
}
