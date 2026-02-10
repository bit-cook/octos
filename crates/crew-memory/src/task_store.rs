//! Task state persistence for resume functionality.

use std::path::{Path, PathBuf};

use crew_core::{Message, Task, TaskId, TokenUsage};
use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

/// Saved state of a running or completed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    /// The task being executed.
    pub task: Task,
    /// Conversation messages so far.
    pub messages: Vec<Message>,
    /// Files modified so far.
    pub files_modified: Vec<PathBuf>,
    /// Token usage so far.
    pub token_usage: TokenUsage,
    /// Whether this is a coordinator task.
    pub is_coordinator: bool,
}

impl TaskState {
    /// Create a new task state.
    pub fn new(task: Task, is_coordinator: bool) -> Self {
        Self {
            task,
            messages: Vec::new(),
            files_modified: Vec::new(),
            token_usage: TokenUsage::default(),
            is_coordinator,
        }
    }
}

/// Store for persisting task state.
pub struct TaskStore {
    /// Directory where task states are stored.
    tasks_dir: PathBuf,
}

impl TaskStore {
    /// Open or create a task store in the given directory.
    pub async fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        let tasks_dir = data_dir.as_ref().join("tasks");
        tokio::fs::create_dir_all(&tasks_dir)
            .await
            .wrap_err("failed to create tasks directory")?;
        Ok(Self { tasks_dir })
    }

    /// Save a task state.
    pub async fn save(&self, state: &TaskState) -> Result<()> {
        let path = self.task_path(&state.task.id);
        let json =
            serde_json::to_string_pretty(state).wrap_err("failed to serialize task state")?;
        tokio::fs::write(&path, json)
            .await
            .wrap_err_with(|| format!("failed to write task state: {}", path.display()))?;
        Ok(())
    }

    /// Load a task state by ID.
    pub async fn load(&self, task_id: &TaskId) -> Result<Option<TaskState>> {
        let path = self.task_path(task_id);
        if !path.exists() {
            return Ok(None);
        }
        let json = tokio::fs::read_to_string(&path)
            .await
            .wrap_err_with(|| format!("failed to read task state: {}", path.display()))?;
        let state: TaskState =
            serde_json::from_str(&json).wrap_err("failed to parse task state")?;
        Ok(Some(state))
    }

    /// List all saved task states.
    pub async fn list(&self) -> Result<Vec<TaskState>> {
        let mut states = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.tasks_dir)
            .await
            .wrap_err("failed to read tasks directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(json) = tokio::fs::read_to_string(&path).await {
                    if let Ok(state) = serde_json::from_str::<TaskState>(&json) {
                        states.push(state);
                    }
                }
            }
        }

        // Sort by updated_at descending (most recent first)
        states.sort_by(|a, b| b.task.updated_at.cmp(&a.task.updated_at));
        Ok(states)
    }

    /// Delete a task state.
    pub async fn delete(&self, task_id: &TaskId) -> Result<()> {
        let path = self.task_path(task_id);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .wrap_err("failed to delete task state")?;
        }
        Ok(())
    }

    fn task_path(&self, task_id: &TaskId) -> PathBuf {
        self.tasks_dir.join(format!("{}.json", task_id))
    }
}
