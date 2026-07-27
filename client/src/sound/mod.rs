use audio::{AudioListener, AudioSettings, FmodStudio, set_global_parameter};
pub use audio::{AudioOutputDevices, UI_BACK_EVENT, UI_CLICK_EVENT, queue_ui_sound};
use bevy::prelude::*;
use gameplay::{components::atmosphere::AreaReverbComponent, pawn::Controller};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};

use crate::settings::Settings;

const GLOBAL_ATMOSPHERE_REVERB: &str = "GlobalAtmosphereReverb";
const IN_SPACE_FILTER: &str = "InSpaceFilter";

pub struct ClientSoundPlugin;

impl Plugin for ClientSoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            sync_audio_settings.run_if(resource_changed::<Settings>),
        )
        .add_systems(
            PostUpdate,
            sync_listener.run_if(resource_exists::<FmodStudio>),
        )
        .add_systems(
            PostUpdate,
            update_atmosphere_reverb.run_if(resource_exists::<FmodStudio>),
        );
    }
}

fn sync_audio_settings(settings: Res<Settings>, mut audio: ResMut<AudioSettings>) {
    audio.output_device = settings.audio_output_device.clone();
    audio.buffer_size = settings.fmod_buffer_size;
}

fn sync_listener(
    camera: Query<&GlobalTransform, With<Camera3d>>,
    possessed: Query<&RigidBodyHandleComponent, With<Controller>>,
    world: Res<PhysicsWorld>,
    mut listener: ResMut<AudioListener>,
) {
    let Ok(gt) = camera.single() else {
        listener.active = false;
        return;
    };
    let (_, rot, pos) = gt.to_scale_rotation_translation();
    listener.position = pos;
    listener.rotation = rot;
    listener.velocity = possessed
        .single()
        .ok()
        .and_then(|h| world.rigid_body_set.get(h.0))
        .map(|rb| {
            let v = rb.linvel();
            Vec3::new(v.x, v.y, v.z)
        })
        .unwrap_or(Vec3::ZERO);
    listener.active = true;
}

fn update_atmosphere_reverb(
    fmod: Res<FmodStudio>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    atmospheres: Query<(&AreaReverbComponent, &GlobalTransform)>,
) {
    let Ok(cam_gt) = camera.single() else {
        return;
    };
    let listener_pos = cam_gt.translation();
    let blend = atmospheres
        .iter()
        .map(|(reverb, gt)| {
            let dist = listener_pos.distance(gt.translation());
            1.0 - ((dist - reverb.min_distance) / (reverb.max_distance - reverb.min_distance))
                .clamp(0.0, 1.0)
        })
        .fold(0.0_f32, f32::max);
    set_global_parameter(&fmod, GLOBAL_ATMOSPHERE_REVERB, blend);
    set_global_parameter(&fmod, IN_SPACE_FILTER, 1.0 - blend);
}
