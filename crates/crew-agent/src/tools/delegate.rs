//! Delegate task tool for coordinator agents.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use crew_core::{AgentId, AgentRole, Task, TaskContext, TaskKind, TokenUsage};
use crew_llm::LlmProvider;
use crew_memory::EpisodeStore;
use eyre::{Result, WrapErr};
use serde::Deserialize;

use super::{Tool, ToolRegistry, ToolResult};
use crate::Agent;

/// Tool for delegating subtasks to worker agents.
pub struct DelegateTaskTool {
    llm: Arc<dyn LlmProvider>,
    memory: Arc<EpisodeStore>,
    working_dir: PathBuf,
    worker_count: std::sync::atomic::AtomicU32,
}

impl DelegateTaskTool {
    /// Create a new delegate task tool.
    pub fn new(llm: Arc<dyn LlmProvider>, memory: Arc<EpisodeStore>, working_dir: PathBuf) -> Self {
        Self {
            llm,
            memory,
            working_dir,
            worker_count: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

#[derive(Debug, Deserialize)]
struct DelegateInput {
    /// Description of the subtask to delegate.
    task: String,
    /// Optional list of files relevant to this subtask.
    #[serde(default)]
    files: Vec<String>,
}

#[async_trait]
impl Tool for DelegateTaskTool {
    fn name(&self) -> &str {
        "delegate_task"
    }

    fn description(&self) -> &str {
        "Delegate a subtask to a worker agent. The worker will execute the task and return results. Use this to break down complex goals into smaller, focused tasks."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Clear description of what the worker should accomplish"
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of file paths relevant to this subtask (optional)"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: DelegateInput =
            serde_json::from_value(args.clone()).wrap_err("invalid delegate_task input")?;

        // Generate worker ID
        let worker_num = self
            .worker_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let worker_id = AgentId::new(format!("worker-{}", worker_num));

        tracing::info!(worker_id = %worker_id, task = %input.task, "delegating subtask");

        // Create worker agent with same LLM and memory
        let tools = ToolRegistry::with_builtins(&self.working_dir);
        let worker = Agent::new(
            worker_id.clone(),
            AgentRole::Worker,
            self.llm.clone(),
            tools,
            self.memory.clone(),
        );

        // Create subtask
        let files: Vec<PathBuf> = input.files.iter().map(PathBuf::from).collect();
        let subtask = Task::new(
            TaskKind::Code {
                instruction: input.task.clone(),
                files,
            },
            TaskContext {
                working_dir: self.working_dir.clone(),
                ..Default::default()
            },
        );

        // Execute subtask with worker
        let result = worker.run_task(&subtask).await?;

        // Format result for coordinator
        let status = if result.success { "SUCCESS" } else { "FAILED" };
        let files_modified = if result.files_modified.is_empty() {
            String::new()
        } else {
            format!(
                "\nFiles modified: {}",
                result
                    .files_modified
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        let output = format!(
            "[Worker {} - {}]\nTask: {}\n\nResult:\n{}{}\n\nTokens: {} input, {} output",
            worker_id,
            status,
            input.task,
            result.output,
            files_modified,
            result.token_usage.input_tokens,
            result.token_usage.output_tokens,
        );

        Ok(ToolResult {
            output,
            success: result.success,
            file_modified: None, // Workers track their own files
            tokens_used: Some(TokenUsage {
                input_tokens: result.token_usage.input_tokens,
                output_tokens: result.token_usage.output_tokens,
            }),
        })
    }
}
