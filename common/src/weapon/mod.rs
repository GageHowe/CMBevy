use crate::weapon::weapon::WeaponStats;

pub mod weapon;
pub mod rifle;

pub const STATS: WeaponStats = WeaponStats {
    damage: 25.0,
    range: 500.0,
};