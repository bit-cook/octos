//! CLI commands for crew-rs.

mod clean;
mod completions;
mod gateway;
mod init;
mod list;
mod resume;
mod run;
mod status;

use clap::{Parser, Subcommand};
use eyre::Result;

pub use clean::CleanCommand;
pub use completions::CompletionsCommand;
pub use gateway::GatewayCommand;
pub use init::InitCommand;
pub use list::ListCommand;
pub use resume::ResumeCommand;
pub use run::RunCommand;
pub use status::StatusCommand;

/// crew-rs: Rust-native coding agent orchestration.
#[derive(Debug, Parser)]
#[command(name = "crew")]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

/// Available commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a new .crew configuration.
    Init(InitCommand),
    /// Run a task with an agent.
    Run(RunCommand),
    /// Resume an interrupted task.
    Resume(ResumeCommand),
    /// List resumable tasks.
    List(ListCommand),
    /// Show details of a specific task.
    Status(StatusCommand),
    /// Run as a persistent messaging gateway.
    Gateway(GatewayCommand),
    /// Clean up stale task state and cache files.
    Clean(CleanCommand),
    /// Generate shell completions.
    Completions(CompletionsCommand),
}

/// Trait for executable commands (following dora-rs pattern).
pub trait Executable {
    fn execute(self) -> Result<()>;
}

impl Executable for Command {
    fn execute(self) -> Result<()> {
        match self {
            Self::Init(cmd) => cmd.execute(),
            Self::Run(cmd) => cmd.execute(),
            Self::Resume(cmd) => cmd.execute(),
            Self::List(cmd) => cmd.execute(),
            Self::Status(cmd) => cmd.execute(),
            Self::Gateway(cmd) => cmd.execute(),
            Self::Clean(cmd) => cmd.execute(),
            Self::Completions(cmd) => cmd.execute(),
        }
    }
}
