pub mod dash;
pub mod jetpack;

pub use dash::DashAbility;
pub use jetpack::JetpackAbility;

pub use super::AbilityPickup;

/// Type alias so spawn.rs can reference these types as a plain path
pub type DashPickup = AbilityPickup<DashAbility>;
pub type JetpackPickup = AbilityPickup<JetpackAbility>;
