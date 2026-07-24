use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct AutoExposureCorrection(pub Option<f32>);

pub struct AutoExposureDebugPlugin;

impl Plugin for AutoExposureDebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AutoExposureCorrection>();
    }
}
