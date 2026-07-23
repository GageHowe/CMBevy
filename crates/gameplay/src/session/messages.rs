#[cfg(feature = "client")]
pub use crate::session::messages_client::draw_server_state;
#[cfg(feature = "client")]
pub(crate) use crate::session::messages_client::{on_message, retry_weapon_pickups};
#[cfg(not(feature = "client"))]
pub use crate::session::messages_server::on_message;
