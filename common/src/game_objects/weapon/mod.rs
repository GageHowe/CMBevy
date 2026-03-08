use bevy::prelude::*;

pub mod weapon;
pub mod rifle;
pub mod shotgun;

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, (
            weapon::fire_weapons::<rifle::RifleComponent>(rifle::apply_rifle_fire),
            weapon::fire_weapons::<shotgun::ShotgunComponent>(shotgun::apply_shotgun_fire),
        ));
    }
}