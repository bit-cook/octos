//! Status command: show system status or task details.

use std::path::PathBuf;

use clap::Args;
use colored::Colorize;
use crew_core::TaskId;
use crew_memory::TaskStore;
use eyre::{Result, WrapErr};

use super::Executable;
use crate::config::Config;

/// Show system status or details of a specific task.
#[derive(Debug, Args)]
pub struct StatusCommand {
    /// Task ID to show details for. Omit to show system status.
    pub task_id: Option<String>,

    /// Working directory (defaults to current directory).
    #[arg(short, long)]
    pub cwd: Option<PathBuf>,
}

impl Executable for StatusCommand {
    fn execute(self) -> Result<()> {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .wrap_err("failed to create tokio runtime")?
            .block_on(self.run_async())
    }
}

/// Known provider environment variable names.
const PROVIDER_ENV_VARS: &[(&str, &str)] = &[
    ("Anthropic", "ANTHROPIC_API_KEY"),
    ("OpenAI", "OPENAI_API_KEY"),
    ("Gemini", "GEMINI_API_KEY"),
    ("OpenRouter", "OPENROUTER_API_KEY"),
    ("DeepSeek", "DEEPSEEK_API_KEY"),
    ("Groq", "GROQ_API_KEY"),
    ("Moonshot", "MOONSHOT_API_KEY"),
    ("DashScope", "DASHSCOPE_API_KEY"),
    ("MiniMax", "MINIMAX_API_KEY"),
    ("Zhipu", "ZHIPU_API_KEY"),
];

impl StatusCommand {
    async fn run_async(self) -> Result<()> {
        let cwd = self.cwd.unwrap_or_else(|| std::env::current_dir().unwrap());

        match self.task_id {
            Some(task_id) => self::show_task_status(&cwd, &task_id).await,
            None => show_system_status(&cwd),
        }
    }
}

fn show_system_status(cwd: &std::path::Path) -> Result<()> {
    println!("{}", "crew-rs Status".cyan().bold());
    println!("{}", "═".repeat(50));
    println!();

    let config_path = cwd.join(".crew").join("config.json");
    let global_config = Config::global_config_path();
    let data_dir = cwd.join(".crew");

    // Config location
    if config_path.exists() {
        println!("{}: {} {}", "Config".green(), config_path.display(), "(found)".green());
    } else if let Some(ref gp) = global_config {
        if gp.exists() {
            println!("{}: {} {}", "Config".green(), gp.display(), "(found)".green());
        } else {
            println!("{}: {}", "Config".yellow(), "not found (run 'crew init')".dimmed());
        }
    } else {
        println!("{}: {}", "Config".yellow(), "not found (run 'crew init')".dimmed());
    }

    // Workspace
    if data_dir.exists() {
        println!("{}: {} {}", "Workspace".green(), data_dir.display(), "(found)".green());
    } else {
        println!("{}: {}", "Workspace".yellow(), "not initialized".dimmed());
    }

    // Load config for provider/model info
    let config = Config::load(cwd).unwrap_or_default();

    let provider = config.provider.as_deref().unwrap_or("anthropic");
    let model = config.model.as_deref().unwrap_or("claude-sonnet-4-20250514");
    println!("{}: {}", "Provider".green(), provider);
    println!("{}: {}", "Model".green(), model);

    if let Some(ref url) = config.base_url {
        println!("{}: {}", "Base URL".green(), url);
    }

    // API keys
    println!();
    println!("{}", "API Keys".cyan().bold());
    println!("{}", "─".repeat(50).dimmed());

    for (label, env_var) in PROVIDER_ENV_VARS {
        let status = if std::env::var(env_var).is_ok() {
            "set".green().to_string()
        } else {
            "not set".dimmed().to_string()
        };
        println!("  {:<12} {:<24} {}", label, env_var.dimmed(), status);
    }

    // Bootstrap files
    println!();
    println!("{}", "Bootstrap Files".cyan().bold());
    println!("{}", "─".repeat(50).dimmed());

    for name in &["AGENTS.md", "SOUL.md", "USER.md", "TOOLS.md", "IDENTITY.md"] {
        let path = data_dir.join(name);
        let status = if path.exists() {
            "found".green().to_string()
        } else {
            "missing".dimmed().to_string()
        };
        println!("  {:<16} {}", name, status);
    }

    // Gateway config
    if let Some(ref gw) = config.gateway {
        println!();
        println!("{}", "Gateway".cyan().bold());
        println!("{}", "─".repeat(50).dimmed());
        println!(
            "  {}: {}",
            "Channels".dimmed(),
            gw.channels
                .iter()
                .map(|c| c.channel_type.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!("  {}: {}", "Max history".dimmed(), gw.max_history);
    }

    println!();

    Ok(())
}

async fn show_task_status(cwd: &std::path::Path, task_id_str: &str) -> Result<()> {
    let data_dir = cwd.join(".crew");
    let task_store = TaskStore::open(&data_dir).await?;

    let task_id: TaskId = task_id_str.parse().wrap_err("invalid task ID format")?;

    let state = task_store
        .load(&task_id)
        .await?
        .ok_or_else(|| eyre::eyre!("task not found: {}", task_id))?;

    // Header
    println!("{}", "Task Details".green().bold());
    println!("{}", "═".repeat(60));
    println!();

    // Task ID
    println!("{}: {}", "ID".cyan().bold(), state.task.id);

    // Status
    let status_str = match &state.task.status {
        crew_core::TaskStatus::Pending => "Pending".yellow(),
        crew_core::TaskStatus::InProgress { agent_id } => format!("In Progress ({})", agent_id)
            .blue()
            .to_string()
            .into(),
        crew_core::TaskStatus::Blocked { reason } => {
            format!("Blocked: {}", reason).red().to_string().into()
        }
        crew_core::TaskStatus::Completed => "Completed".green(),
        crew_core::TaskStatus::Failed { error } => {
            format!("Failed: {}", error).red().to_string().into()
        }
    };
    println!("{}: {}", "Status".cyan().bold(), status_str);

    // Role
    let role = if state.is_coordinator {
        "Coordinator".cyan()
    } else {
        "Worker".blue()
    };
    println!("{}: {}", "Role".cyan().bold(), role);

    // Task kind
    println!();
    println!("{}", "Task".cyan().bold());
    println!("{}", "─".repeat(60).dimmed());
    match &state.task.kind {
        crew_core::TaskKind::Code { instruction, files } => {
            println!("{}: Code", "Type".dimmed());
            println!("{}: {}", "Instruction".dimmed(), instruction);
            if !files.is_empty() {
                println!(
                    "{}: {}",
                    "Files".dimmed(),
                    files
                        .iter()
                        .map(|f| f.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        crew_core::TaskKind::Plan { goal } => {
            println!("{}: Plan", "Type".dimmed());
            println!("{}: {}", "Goal".dimmed(), goal);
        }
        crew_core::TaskKind::Review { diff } => {
            println!("{}: Review", "Type".dimmed());
            let preview = if diff.len() > 200 {
                format!("{}...", &diff[..200])
            } else {
                diff.clone()
            };
            println!("{}: {}", "Diff preview".dimmed(), preview);
        }
        crew_core::TaskKind::Test { command } => {
            println!("{}: Test", "Type".dimmed());
            println!("{}: {}", "Command".dimmed(), command);
        }
        crew_core::TaskKind::Custom { name, params } => {
            println!("{}: Custom ({})", "Type".dimmed(), name);
            println!("{}: {}", "Params".dimmed(), params);
        }
    }

    // Progress
    println!();
    println!("{}", "Progress".cyan().bold());
    println!("{}", "─".repeat(60).dimmed());
    println!(
        "{}: {} input, {} output",
        "Tokens used".dimmed(),
        state.token_usage.input_tokens,
        state.token_usage.output_tokens
    );
    println!("{}: {}", "Messages".dimmed(), state.messages.len());
    println!(
        "{}: {}",
        "Files modified".dimmed(),
        state.files_modified.len()
    );

    if !state.files_modified.is_empty() {
        println!();
        println!("{}", "Modified files:".dimmed());
        for file in &state.files_modified {
            println!("  - {}", file.display());
        }
    }

    // Timestamps
    println!();
    println!("{}", "Timestamps".cyan().bold());
    println!("{}", "─".repeat(60).dimmed());
    println!(
        "{}: {}",
        "Created".dimmed(),
        state.task.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "{}: {}",
        "Updated".dimmed(),
        state.task.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

    // Conversation preview
    if !state.messages.is_empty() {
        println!();
        println!("{}", "Conversation (last 3 messages)".cyan().bold());
        println!("{}", "─".repeat(60).dimmed());

        let start = state.messages.len().saturating_sub(3);
        for msg in &state.messages[start..] {
            let role_str = match msg.role {
                crew_core::MessageRole::System => "System".magenta(),
                crew_core::MessageRole::User => "User".green(),
                crew_core::MessageRole::Assistant => "Assistant".blue(),
                crew_core::MessageRole::Tool => "Tool".yellow(),
            };

            let content_preview = if msg.content.len() > 100 {
                format!("{}...", &msg.content[..100])
            } else {
                msg.content.clone()
            };

            println!();
            println!("{}: {}", role_str, content_preview.replace('\n', " "));
        }
    }

    println!();
    println!("{}", "─".repeat(60).dimmed());
    println!(
        "{}",
        format!("Run 'crew resume {}' to continue this task.", task_id).dimmed()
    );

    Ok(())
}
