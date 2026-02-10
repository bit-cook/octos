//! LLM provider abstraction for crew-rs.
//!
//! This crate provides a unified interface for interacting with LLM providers:
//! - Anthropic (Claude)
//! - OpenAI (GPT-4)
//! - Google Gemini
//! - Ollama (local models)

mod config;
mod provider;
mod retry;
mod types;

pub mod anthropic;
pub mod gemini;
pub mod openai;

pub use config::ChatConfig;
pub use provider::LlmProvider;
pub use retry::{RetryConfig, RetryProvider};
pub use types::{ChatResponse, StopReason, TokenUsage, ToolSpec};
