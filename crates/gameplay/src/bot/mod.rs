mod brains;
#[cfg(not(feature = "client"))]
mod server;
mod types;

pub use brains::HeuristicKillerBot;
#[cfg(not(feature = "client"))]
pub use server::*;
pub use types::{BotBehavior, BotContext, BotController, BotOutput, collect_contexts};
