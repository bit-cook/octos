//! Resume command: continue an interrupted task.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Args;
use colored::Colorize;
use crew_agent::{Agent, AgentConfig, ConsoleReporter, ToolRegistry};
use crew_core::{AgentId, AgentRole, TaskId};
use crew_llm::{
    LlmProvider, RetryProvider, anthropic::AnthropicProvider, gemini::GeminiProvider,
    openai::OpenAIProvider,
};
use crew_memory::{EpisodeStore, TaskStore};
use eyre::{Result, WrapErr};
use tracing::info;

use super::Executable;
use crate::config::Config;

/// Resume an interrupted task.
#[derive(Debug, Args)]
pub struct ResumeCommand {
    /// Task ID to resume (optional - shows list if not provided).
    pub task_id: Option<String>,

    /// Working directory (defaults to current directory).
    #[arg(short, long)]
    pub cwd: Option<PathBuf>,

    /// Path to config file.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// LLM provider to use (overrides config).
    #[arg(long)]
    pub provider: Option<String>,

    /// Model to use (overrides config).
    #[arg(long)]
    pub model: Option<String>,

    /// Custom base URL for the API endpoint (overrides config).
    #[arg(long)]
    pub base_url: Option<String>,

    /// Maximum number of iterations (default: 50).
    #[arg(long, default_value = "50")]
    pub max_iterations: u32,

    /// Maximum total tokens (input + output). Unlimited if not set.
    #[arg(long)]
    pub max_tokens: Option<u32>,

    /// Verbose output (show tool outputs).
    #[arg(short, long)]
    pub verbose: bool,

    /// Disable automatic retry on transient errors.
    #[arg(long)]
    pub no_retry: bool,
}

impl Executable for ResumeCommand {
    fn execute(self) -> Result<()> {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .wrap_err("failed to create tokio runtime")?
            .block_on(self.run_async())
    }
}

impl ResumeCommand {
    async fn run_async(self) -> Result<()> {
        println!("{}", "crew-rs".cyan().bold());
        println!();

        let cwd = self.cwd.unwrap_or_else(|| std::env::current_dir().unwrap());

        // Open task store
        let data_dir = cwd.join(".crew");
        let task_store = TaskStore::open(&data_dir).await?;

        // If no task ID provided, list available tasks
        let task_id = match self.task_id {
            Some(id) => id,
            None => {
                let tasks = task_store.list().await?;
                if tasks.is_empty() {
                    println!("{}", "No resumable tasks found.".yellow());
                    return Ok(());
                }

                println!("{}", "Resumable tasks:".green());
                println!();
                for state in &tasks {
                    let kind_str = match &state.task.kind {
                        crew_core::TaskKind::Code { instruction, .. } => {
                            if instruction.len() > 50 {
                                format!("{}...", &instruction[..50])
                            } else {
                                instruction.clone()
                            }
                        }
                        crew_core::TaskKind::Plan { goal } => format!("Plan: {}", goal),
                        _ => "Task".to_string(),
                    };
                    let role = if state.is_coordinator {
                        "coordinator"
                    } else {
                        "worker"
                    };
                    println!(
                        "  {} {} [{}]",
                        state.task.id.to_string().cyan(),
                        kind_str,
                        role.dimmed()
                    );
                    println!(
                        "    {} {} input, {} output tokens",
                        "Progress:".dimmed(),
                        state.token_usage.input_tokens,
                        state.token_usage.output_tokens
                    );
                    println!(
                        "    {} {}",
                        "Updated:".dimmed(),
                        state.task.updated_at.format("%Y-%m-%d %H:%M:%S")
                    );
                    println!();
                }
                println!(
                    "{}",
                    "Run 'crew resume <task-id>' to continue a task.".dimmed()
                );
                return Ok(());
            }
        };

        // Parse task ID
        let task_id: TaskId = task_id.parse().wrap_err("invalid task ID format")?;

        // Load task state
        let state = task_store
            .load(&task_id)
            .await?
            .ok_or_else(|| eyre::eyre!("task not found: {}", task_id))?;

        info!(task_id = %task_id, "resuming task");

        println!("{}: {}", "Resuming task".green(), task_id);
        println!("{}: {}", "Working dir".green(), cwd.display());

        // Load config
        let config = if let Some(config_path) = &self.config {
            Config::from_file(config_path)?
        } else {
            Config::load(&cwd)?
        };

        // Merge CLI args with config
        let provider = self
            .provider
            .or(config.provider.clone())
            .unwrap_or_else(|| "anthropic".to_string());
        let model = self.model.or(config.model.clone());
        let base_url = self.base_url.or(config.base_url.clone());

        println!("{}: {}", "Provider".green(), provider);

        // Create LLM provider
        let base_provider: Arc<dyn LlmProvider> = match provider.as_str() {
            "anthropic" => {
                let api_key = config.get_api_key("anthropic")?;
                let model_name = model.unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());
                let mut provider = AnthropicProvider::new(&api_key, &model_name);
                if let Some(url) = &base_url {
                    provider = provider.with_base_url(url);
                }
                println!("{}: {}", "Model".green(), provider.model_id());
                Arc::new(provider)
            }
            "openai" => {
                let api_key = config.get_api_key("openai")?;
                let model_name = model.unwrap_or_else(|| "gpt-4o".to_string());
                let mut provider = OpenAIProvider::new(&api_key, &model_name);
                if let Some(url) = &base_url {
                    provider = provider.with_base_url(url);
                }
                println!("{}: {}", "Model".green(), provider.model_id());
                Arc::new(provider)
            }
            "gemini" | "google" => {
                let api_key = config.get_api_key("gemini")?;
                let model_name = model.unwrap_or_else(|| "gemini-2.0-flash".to_string());
                let mut provider = GeminiProvider::new(&api_key, &model_name);
                if let Some(url) = &base_url {
                    provider = provider.with_base_url(url);
                }
                println!("{}: {}", "Model".green(), provider.model_id());
                Arc::new(provider)
            }
            other => {
                eyre::bail!(
                    "unknown provider: {}. Use 'anthropic', 'openai', or 'gemini'",
                    other
                );
            }
        };

        // Wrap with retry unless disabled
        let llm: Arc<dyn LlmProvider> = if self.no_retry {
            base_provider
        } else {
            Arc::new(RetryProvider::new(base_provider))
        };

        // Create memory and tools
        let memory = Arc::new(EpisodeStore::open(&data_dir).await?);
        let role = if state.is_coordinator {
            AgentRole::Coordinator
        } else {
            AgentRole::Worker
        };

        let tools = if state.is_coordinator {
            ToolRegistry::with_coordinator_tools(&cwd, llm.clone(), memory.clone())
        } else {
            ToolRegistry::with_builtins(&cwd)
        };

        println!("{}: {:?}", "Role".green(), role);
        println!(
            "{}: {} input, {} output",
            "Previous tokens".green(),
            state.token_usage.input_tokens,
            state.token_usage.output_tokens
        );
        println!("{}: {}", "Max iterations".green(), self.max_iterations);
        if let Some(max_tokens) = self.max_tokens {
            println!("{}: {}", "Token budget".green(), max_tokens);
        }
        println!();
        println!("{}", "─".repeat(60).dimmed());
        println!();

        // Set up Ctrl+C handler
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            if let Ok(()) = tokio::signal::ctrl_c().await {
                println!();
                println!("{}", "Received Ctrl+C, saving state...".yellow());
                shutdown_clone.store(true, Ordering::Relaxed);
            }
        });

        // Create progress reporter
        let reporter = Arc::new(ConsoleReporter::new().with_verbose(self.verbose));

        // Create agent config
        let agent_config = AgentConfig {
            max_iterations: self.max_iterations,
            max_tokens: self.max_tokens,
            save_episodes: true,
        };

        // Create agent
        let agent = Agent::new(AgentId::new("agent-1"), role, llm, tools, memory)
            .with_config(agent_config)
            .with_reporter(reporter)
            .with_shutdown(shutdown);

        // Resume task
        println!("{}", "(Ctrl+C to interrupt, state will be saved)".dimmed());
        println!();

        let task = state.task.clone();
        let result = agent
            .run_task_resumable(&task, &task_store, Some(state))
            .await?;

        println!();
        println!("{}", "─".repeat(60).dimmed());
        println!();

        if result.success {
            println!("{}", "Task completed successfully!".green().bold());
        } else {
            println!("{}", "Task failed.".red().bold());
        }

        println!();
        println!("{}", "Output:".cyan());
        println!("{}", result.output);

        println!();
        println!(
            "{}: {} input, {} output",
            "Total tokens".dimmed(),
            result.token_usage.input_tokens,
            result.token_usage.output_tokens
        );

        if !result.files_modified.is_empty() {
            println!();
            println!("{}", "Files modified:".cyan());
            for file in &result.files_modified {
                println!("  - {}", file.display());
            }
        }

        Ok(())
    }
}
