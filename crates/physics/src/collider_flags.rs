use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ColliderFlags: u128 {
        const SHIELD = 1u128 << 0; // one-way or two-way shields. checked by projectiles
        const BIPED = 1u128 << 1; // not sure what we could use this for yet, but it may be useful eventually
        const PLANET = 1u128 << 2; // so planets can check for other planets. this is useful when we want to avoid appling planet gravity twice
        const PROJECTILE_IMMUNE = 1u128 << 3; // doesn't react to projectile raycasts at all
    }
}

pub fn collider_flags(user_data: u128) -> ColliderFlags {
    ColliderFlags::from_bits_truncate(user_data)
}
