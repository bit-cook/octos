//! List command: show resumable tasks.

use std::path::PathBuf;

use clap::Args;
use colored::Colorize;
use crew_memory::TaskStore;
use eyre::{Result, WrapErr};

use super::Executable;

/// List resumable tasks.
#[derive(Debug, Args)]
pub struct ListCommand {
    /// Working directory (defaults to current directory).
    #[arg(short, long)]
    pub cwd: Option<PathBuf>,
}

impl Executable for ListCommand {
    fn execute(self) -> Result<()> {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .wrap_err("failed to create tokio runtime")?
            .block_on(self.run_async())
    }
}

impl ListCommand {
    async fn run_async(self) -> Result<()> {
        let cwd = self.cwd.unwrap_or_else(|| std::env::current_dir().unwrap());

        let data_dir = cwd.join(".crew");
        let task_store = TaskStore::open(&data_dir).await?;
        let tasks = task_store.list().await?;

        if tasks.is_empty() {
            println!("{}", "No resumable tasks found.".yellow());
            return Ok(());
        }

        println!("{}", "Resumable tasks:".green().bold());
        println!();

        for state in &tasks {
            let kind_str = match &state.task.kind {
                crew_core::TaskKind::Code { instruction, .. } => {
                    if instruction.len() > 60 {
                        format!("{}...", &instruction[..60])
                    } else {
                        instruction.clone()
                    }
                }
                crew_core::TaskKind::Plan { goal } => {
                    if goal.len() > 60 {
                        format!("Plan: {}...", &goal[..57])
                    } else {
                        format!("Plan: {}", goal)
                    }
                }
                crew_core::TaskKind::Review { .. } => "Review".to_string(),
                crew_core::TaskKind::Test { command } => format!("Test: {}", command),
                crew_core::TaskKind::Custom { name, .. } => format!("Custom: {}", name),
            };

            let role = if state.is_coordinator {
                "coordinator".cyan()
            } else {
                "worker".blue()
            };

            println!("{}", state.task.id.to_string().yellow().bold());
            println!("  {} {}", "Task:".dimmed(), kind_str);
            println!("  {} {}", "Role:".dimmed(), role);
            println!(
                "  {} {} input, {} output",
                "Tokens:".dimmed(),
                state.token_usage.input_tokens,
                state.token_usage.output_tokens
            );
            println!(
                "  {} {} files",
                "Modified:".dimmed(),
                state.files_modified.len()
            );
            println!(
                "  {} {}",
                "Updated:".dimmed(),
                state.task.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            println!();
        }

        println!(
            "{}",
            "Run 'crew resume <task-id>' to continue a task.".dimmed()
        );

        Ok(())
    }
}
