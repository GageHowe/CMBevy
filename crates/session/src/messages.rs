#[cfg(feature = "client")]
pub use crate::messages_client::draw_server_state;
#[cfg(feature = "client")]
pub(crate) use crate::messages_client::on_message;

#[cfg(not(feature = "client"))]
pub use crate::messages_server::on_message;
