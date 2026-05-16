use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ColliderFlags: u128 {
        const SHIELD = 1 << 0;
    }
}

pub fn collider_flags(user_data: u128) -> ColliderFlags {
    ColliderFlags::from_bits_truncate(user_data)
}
