use bevy::prelude::*;

#[derive(Resource, Clone, Copy)]
pub struct Tick{
    pub tick: u64
}

pub struct TickPlugin;
impl Plugin for TickPlugin
{
    fn build(&self, app: &mut App) {
        app.insert_resource(Tick{
            tick: 0
        });
        // app.add_systems(FixedUpdate, increment_tick);
    }
}

pub fn increment_tick(mut tick: ResMut<Tick>){
    tick.tick += 1;
}