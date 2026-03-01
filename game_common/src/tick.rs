use bevy::prelude::*;

#[derive(Resource, Clone, Copy)]
pub struct Ticker {
    pub tick: u64
}

// pub struct TickPlugin;
// impl Plugin for TickPlugin
// {
//     fn build(&self, app: &mut App) {
//         app.insert_resource(Ticker {
//             tick: 0
//         });
//         
//         // not gonna add the system here, add it in master_plugin
//         // app.add_systems(FixedUpdate, increment_tick);
//     }
// }

pub fn increment_tick(mut tick: ResMut<Ticker>){
    tick.tick += 1;
}