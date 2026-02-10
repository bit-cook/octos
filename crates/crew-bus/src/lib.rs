//! Message bus, channels, and session management for crew-rs gateway.

pub mod bus;
pub mod channel;
pub mod cli_channel;
pub mod session;

pub use bus::{AgentHandle, BusPublisher, create_bus};
pub use channel::{Channel, ChannelManager};
pub use cli_channel::CliChannel;
pub use session::{Session, SessionManager};
