//! Core type definitions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a task (UUID v7 for temporal ordering).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub Uuid);

impl TaskId {
    /// Create a new task ID using UUID v7.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for TaskId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Unique identifier for an agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl AgentId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Agent role in the coordination hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    /// Coordinator: plans and delegates tasks to workers.
    Coordinator,
    /// Worker: executes tasks and reports results.
    Worker,
}

/// A message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Reference to an episode in episodic memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeRef {
    pub id: String,
    pub summary: String,
    pub relevance_score: f32,
}

/// Unique key identifying a conversation session (channel:chat_id).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey(pub String);

impl SessionKey {
    pub fn new(channel: &str, chat_id: &str) -> Self {
        Self(format!("{channel}:{chat_id}"))
    }
}

impl std::fmt::Display for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_id_unique() {
        let id1 = TaskId::new();
        let id2 = TaskId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_task_id_display() {
        let id = TaskId::new();
        let s = id.to_string();
        assert!(!s.is_empty());
        assert!(s.contains('-')); // UUID format
    }

    #[test]
    fn test_task_id_from_str() {
        let id = TaskId::new();
        let s = id.to_string();
        let parsed: TaskId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_agent_id() {
        let id = AgentId::new("worker-1");
        assert_eq!(id.to_string(), "worker-1");
    }

    #[test]
    fn test_agent_role_serialization() {
        let role = AgentRole::Coordinator;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, r#""coordinator""#);

        let parsed: AgentRole = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, AgentRole::Coordinator);
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message {
            role: MessageRole::User,
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("Hello"));
        assert!(json.contains("user"));

        // tool_calls should be skipped when None
        assert!(!json.contains("tool_calls"));
    }

    #[test]
    fn test_session_key_new() {
        let key = SessionKey::new("telegram", "12345");
        assert_eq!(key.0, "telegram:12345");
    }

    #[test]
    fn test_session_key_display() {
        let key = SessionKey::new("cli", "default");
        assert_eq!(key.to_string(), "cli:default");
    }

    #[test]
    fn test_session_key_equality() {
        let k1 = SessionKey::new("cli", "a");
        let k2 = SessionKey::new("cli", "a");
        let k3 = SessionKey::new("cli", "b");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_session_key_serde_roundtrip() {
        let key = SessionKey::new("discord", "guild:123");
        let json = serde_json::to_string(&key).unwrap();
        let parsed: SessionKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, parsed);
    }

    #[test]
    fn test_episode_ref() {
        let ep_ref = EpisodeRef {
            id: "ep-123".to_string(),
            summary: "Fixed auth bug".to_string(),
            relevance_score: 0.85,
        };
        let json = serde_json::to_string(&ep_ref).unwrap();
        assert!(json.contains("ep-123"));
        assert!(json.contains("0.85"));
    }
}
