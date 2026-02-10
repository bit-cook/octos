//! Shell completions command.

use std::io;

use clap::{Args, CommandFactory};
use clap_complete::{Shell, generate};
use eyre::Result;

use super::{Args as CliArgs, Executable};

/// Generate shell completions for crew CLI.
#[derive(Debug, Args)]
pub struct CompletionsCommand {
    /// Shell to generate completions for.
    #[arg(value_enum)]
    pub shell: Shell,
}

impl Executable for CompletionsCommand {
    fn execute(self) -> Result<()> {
        let mut cmd = CliArgs::command();
        generate(self.shell, &mut cmd, "crew", &mut io::stdout());
        Ok(())
    }
}
