//! Parallel batch delegation tool for coordinator agents.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use crew_core::{AgentId, AgentRole, Task, TaskContext, TaskKind, TokenUsage};
use crew_llm::LlmProvider;
use crew_memory::EpisodeStore;
use eyre::{Result, WrapErr};
use futures::future::join_all;
use serde::Deserialize;

use super::{Tool, ToolRegistry, ToolResult};
use crate::Agent;

/// Tool for delegating multiple subtasks to parallel workers.
pub struct DelegateBatchTool {
    llm: Arc<dyn LlmProvider>,
    memory: Arc<EpisodeStore>,
    working_dir: PathBuf,
    worker_count: std::sync::atomic::AtomicU32,
}

impl DelegateBatchTool {
    /// Create a new batch delegation tool.
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
struct SubtaskInput {
    /// Description of the subtask.
    task: String,
    /// Optional list of files relevant to this subtask.
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BatchInput {
    /// List of subtasks to execute in parallel.
    tasks: Vec<SubtaskInput>,
}

#[async_trait]
impl Tool for DelegateBatchTool {
    fn name(&self) -> &str {
        "delegate_batch"
    }

    fn description(&self) -> &str {
        "Delegate multiple subtasks to worker agents in parallel. Use this when you have independent subtasks that can run concurrently. More efficient than sequential delegate_task calls."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "items": {
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
                    },
                    "description": "List of independent subtasks to execute in parallel"
                }
            },
            "required": ["tasks"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: BatchInput =
            serde_json::from_value(args.clone()).wrap_err("invalid delegate_batch input")?;

        if input.tasks.is_empty() {
            return Ok(ToolResult {
                output: "No tasks provided".to_string(),
                success: false,
                ..Default::default()
            });
        }

        let task_count = input.tasks.len();
        tracing::info!(
            count = task_count,
            "delegating batch of subtasks in parallel"
        );

        // Spawn all workers in parallel
        let mut handles = Vec::with_capacity(task_count);

        for subtask_input in input.tasks {
            let worker_num = self
                .worker_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let worker_id = AgentId::new(format!("worker-{}", worker_num));
            let llm = self.llm.clone();
            let memory = self.memory.clone();
            let working_dir = self.working_dir.clone();

            let handle = tokio::spawn(async move {
                let span =
                    tracing::info_span!("worker", id = %worker_id, task = %subtask_input.task);
                let _guard = span.enter();

                tracing::info!("starting parallel worker");

                // Create worker agent
                let tools = ToolRegistry::with_builtins(&working_dir);
                let worker = Agent::new(worker_id.clone(), AgentRole::Worker, llm, tools, memory);

                // Create subtask
                let files: Vec<PathBuf> = subtask_input.files.iter().map(PathBuf::from).collect();
                let task = Task::new(
                    TaskKind::Code {
                        instruction: subtask_input.task.clone(),
                        files,
                    },
                    TaskContext {
                        working_dir,
                        ..Default::default()
                    },
                );

                // Execute
                let result = worker.run_task(&task).await;

                (worker_id, subtask_input.task, result)
            });

            handles.push(handle);
        }

        // Wait for all workers
        let results = join_all(handles).await;

        // Aggregate results
        let mut outputs = Vec::new();
        let mut all_success = true;
        let mut total_input_tokens = 0u32;
        let mut total_output_tokens = 0u32;
        let mut all_files_modified = Vec::new();

        for join_result in results {
            match join_result {
                Ok((worker_id, task_desc, result)) => match result {
                    Ok(task_result) => {
                        let status = if task_result.success {
                            "SUCCESS"
                        } else {
                            "FAILED"
                        };
                        all_success = all_success && task_result.success;
                        total_input_tokens += task_result.token_usage.input_tokens;
                        total_output_tokens += task_result.token_usage.output_tokens;
                        all_files_modified.extend(task_result.files_modified.clone());

                        let files_modified = if task_result.files_modified.is_empty() {
                            String::new()
                        } else {
                            format!(
                                "\nFiles: {}",
                                task_result
                                    .files_modified
                                    .iter()
                                    .map(|p| p.display().to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };

                        outputs.push(format!(
                            "### Worker {} [{}]\nTask: {}\n{}{}\n",
                            worker_id, status, task_desc, task_result.output, files_modified
                        ));
                    }
                    Err(e) => {
                        all_success = false;
                        outputs.push(format!(
                            "### Worker {} [ERROR]\nTask: {}\nError: {}\n",
                            worker_id, task_desc, e
                        ));
                    }
                },
                Err(e) => {
                    all_success = false;
                    outputs.push(format!("### Worker [PANIC]\nError: {}\n", e));
                }
            }
        }

        let output = format!(
            "## Parallel Batch Results\n\n{}\n---\nTotal: {} tasks, {} input tokens, {} output tokens",
            outputs.join("\n---\n\n"),
            task_count,
            total_input_tokens,
            total_output_tokens
        );

        Ok(ToolResult {
            output,
            success: all_success,
            file_modified: None, // Workers track their own
            tokens_used: Some(TokenUsage {
                input_tokens: total_input_tokens,
                output_tokens: total_output_tokens,
            }),
        })
    }
}
