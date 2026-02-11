//! Run command: execute a task with an agent.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Args;
use colored::Colorize;
use crew_agent::{Agent, AgentConfig, ConsoleReporter, ToolRegistry};
use crew_core::{AgentId, AgentRole, Task, TaskContext, TaskKind};
use crew_llm::{
    LlmProvider, RetryProvider, anthropic::AnthropicProvider, gemini::GeminiProvider,
    openai::OpenAIProvider, openrouter::OpenRouterProvider,
};
use crew_memory::{EpisodeStore, TaskStore};
use eyre::{Result, WrapErr};
use tracing::info;

use super::Executable;
use crate::config::Config;

/// Run a task with an agent.
#[derive(Debug, Args)]
pub struct RunCommand {
    /// The goal or instruction to execute.
    #[arg(required = true)]
    pub goal: String,

    /// Working directory (defaults to current directory).
    #[arg(short, long)]
    pub cwd: Option<PathBuf>,

    /// Path to config file (default: .crew/config.json or ~/.config/crew/config.json).
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

    /// Run as coordinator (decompose into subtasks).
    #[arg(long)]
    pub coordinate: bool,

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

impl Executable for RunCommand {
    fn execute(self) -> Result<()> {
        // Use tokio runtime for async execution
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .wrap_err("failed to create tokio runtime")?
            .block_on(self.run_async())
    }
}

impl RunCommand {
    async fn run_async(self) -> Result<()> {
        println!("{}", "crew-rs".cyan().bold());
        println!();

        let cwd = self.cwd.unwrap_or_else(|| std::env::current_dir().unwrap());
        info!(cwd = %cwd.display(), goal = %self.goal, "starting task");

        // Load config (from file or defaults)
        let config = if let Some(config_path) = &self.config {
            Config::from_file(config_path)?
        } else {
            Config::load(&cwd)?
        };

        // Merge CLI args with config (CLI takes precedence)
        let model = self.model.or(config.model.clone());
        let base_url = self.base_url.or(config.base_url.clone());
        let provider = self
            .provider
            .or(config.provider.clone())
            .or_else(|| {
                model
                    .as_deref()
                    .and_then(crate::config::detect_provider)
                    .map(String::from)
            })
            .unwrap_or_else(|| "anthropic".to_string());

        println!("{}: {}", "Goal".green(), self.goal);
        println!("{}: {}", "Working dir".green(), cwd.display());
        println!("{}: {}", "Provider".green(), provider);

        // Create LLM provider
        let base_provider: Arc<dyn LlmProvider> = match provider.as_str() {
            "anthropic" => {
                let api_key = config.get_api_key("anthropic")?;
                let model_name = model.unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());
                let mut provider = AnthropicProvider::new(&api_key, &model_name);
                if let Some(url) = &base_url {
                    provider = provider.with_base_url(url);
                    println!("{}: {}", "Base URL".green(), url);
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
                    println!("{}: {}", "Base URL".green(), url);
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
                    println!("{}: {}", "Base URL".green(), url);
                }
                println!("{}: {}", "Model".green(), provider.model_id());
                Arc::new(provider)
            }
            "openrouter" => {
                let api_key = config.get_api_key("openrouter")?;
                let model_name =
                    model.unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".to_string());
                let mut p = OpenRouterProvider::new(&api_key, &model_name);
                if let Some(url) = &base_url {
                    p = p.with_base_url(url);
                }
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            "deepseek" => {
                let api_key = config.get_api_key("deepseek")?;
                let model_name = model.unwrap_or_else(|| "deepseek-chat".to_string());
                let p = OpenAIProvider::new(&api_key, &model_name).with_base_url(
                    base_url.as_deref().unwrap_or("https://api.deepseek.com/v1"),
                );
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            "groq" => {
                let api_key = config.get_api_key("groq")?;
                let model_name =
                    model.unwrap_or_else(|| "llama-3.3-70b-versatile".to_string());
                let p = OpenAIProvider::new(&api_key, &model_name).with_base_url(
                    base_url.as_deref().unwrap_or("https://api.groq.com/openai/v1"),
                );
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            "moonshot" | "kimi" => {
                let api_key = config.get_api_key("moonshot")?;
                let model_name = model.unwrap_or_else(|| "kimi-k2.5".to_string());
                let p = OpenAIProvider::new(&api_key, &model_name).with_base_url(
                    base_url.as_deref().unwrap_or("https://api.moonshot.ai/v1"),
                );
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            "dashscope" | "qwen" => {
                let api_key = config.get_api_key("dashscope")?;
                let model_name = model.unwrap_or_else(|| "qwen-max".to_string());
                let p = OpenAIProvider::new(&api_key, &model_name).with_base_url(
                    base_url
                        .as_deref()
                        .unwrap_or("https://dashscope.aliyuncs.com/compatible-mode/v1"),
                );
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            "minimax" => {
                let api_key = config.get_api_key("minimax")?;
                let model_name = model.unwrap_or_else(|| "MiniMax-Text-01".to_string());
                let p = OpenAIProvider::new(&api_key, &model_name).with_base_url(
                    base_url.as_deref().unwrap_or("https://api.minimax.io/v1"),
                );
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            "zhipu" | "glm" => {
                let api_key = config.get_api_key("zhipu")?;
                let model_name = model.unwrap_or_else(|| "glm-4-plus".to_string());
                let p = OpenAIProvider::new(&api_key, &model_name).with_base_url(
                    base_url
                        .as_deref()
                        .unwrap_or("https://open.bigmodel.cn/api/paas/v4"),
                );
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            "ollama" => {
                let model_name = model.unwrap_or_else(|| "llama3.2".to_string());
                let p = OpenAIProvider::new("ollama", &model_name).with_base_url(
                    base_url.as_deref().unwrap_or("http://localhost:11434/v1"),
                );
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            "vllm" => {
                let api_key = config
                    .get_api_key("vllm")
                    .unwrap_or_else(|_| "token".to_string());
                let model_name = model.ok_or_else(|| {
                    eyre::eyre!("vllm provider requires --model to be specified")
                })?;
                let url = base_url.ok_or_else(|| {
                    eyre::eyre!("vllm provider requires --base-url to be specified")
                })?;
                let p = OpenAIProvider::new(&api_key, &model_name).with_base_url(&url);
                println!("{}: {}", "Model".green(), p.model_id());
                Arc::new(p)
            }
            other => {
                eyre::bail!(
                    "unknown provider: {other}. Valid: anthropic, openai, gemini, openrouter, \
                     deepseek, groq, moonshot, dashscope, minimax, zhipu, ollama, vllm"
                );
            }
        };

        // Wrap with retry unless disabled
        let llm: Arc<dyn LlmProvider> = if self.no_retry {
            base_provider
        } else {
            println!("{}: enabled (3 attempts)", "Retry".green());
            Arc::new(RetryProvider::new(base_provider))
        };

        println!("{}: {}", "Max iterations".green(), self.max_iterations);
        if let Some(max_tokens) = self.max_tokens {
            println!("{}: {}", "Token budget".green(), max_tokens);
        }
        println!();

        // Create stores
        let data_dir = cwd.join(".crew");
        let memory = Arc::new(
            EpisodeStore::open(&data_dir)
                .await
                .wrap_err("failed to open episode store")?,
        );
        let task_store = TaskStore::open(&data_dir)
            .await
            .wrap_err("failed to open task store")?;

        // Create tool registry based on role
        let role = if self.coordinate {
            AgentRole::Coordinator
        } else {
            AgentRole::Worker
        };

        let tools = if self.coordinate {
            println!(
                "{}: shell, read_file, edit_file, write_file, glob, grep, delegate_task, delegate_batch",
                "Tools".green()
            );
            ToolRegistry::with_coordinator_tools(&cwd, llm.clone(), memory.clone())
        } else {
            println!(
                "{}: shell, read_file, edit_file, write_file, glob, grep",
                "Tools".green()
            );
            ToolRegistry::with_builtins(&cwd)
        };

        println!("{}: {:?}", "Role".green(), role);
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

        // Create task
        let task = Task::new(
            TaskKind::Code {
                instruction: self.goal.clone(),
                files: Vec::new(),
            },
            TaskContext {
                working_dir: cwd,
                ..Default::default()
            },
        );

        // Run task (with state persistence for resume)
        println!(
            "{}",
            "(Ctrl+C to interrupt, 'crew resume' to continue)".dimmed()
        );
        println!();

        let result = agent.run_task_resumable(&task, &task_store, None).await?;

        println!();
        println!("{}", "─".repeat(60).dimmed());
        println!();

        if result.success {
            println!("{}", "Task completed successfully!".green().bold());
        } else {
            println!("{}", "Task stopped.".yellow().bold());
        }

        println!();
        println!("{}", "Output:".cyan());
        println!("{}", result.output);

        println!();
        println!(
            "{}: {} input, {} output",
            "Tokens".dimmed(),
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
