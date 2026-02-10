//! Core types, task model, and protocols for crew-rs.
//!
//! This crate defines the foundational types used across all crew-rs crates:
//! - Task model (Task, TaskStatus, TaskKind)
//! - Agent roles and identifiers
//! - Message protocol between agents
//! - Context and result types

mod error;
pub mod gateway;
mod message;
mod task;
mod types;

pub use error::{Error, ErrorKind, Result};
pub use gateway::{InboundMessage, OutboundMessage};
pub use message::AgentMessage;
pub use task::{Task, TaskContext, TaskKind, TaskResult, TaskStatus, TokenUsage};
pub use types::{
    AgentId, AgentRole, EpisodeRef, Message, MessageRole, SessionKey, TaskId, ToolCall,
};
