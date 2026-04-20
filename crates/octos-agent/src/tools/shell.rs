//! Shell tool for executing commands.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use eyre::{Result, WrapErr};
use serde::Deserialize;
use tokio::time::timeout;

use super::{Tool, ToolResult};
use crate::policy::{CommandPolicy, Decision, SafePolicy};
use crate::sandbox::{NoSandbox, Sandbox};

/// Tool for executing shell commands.
pub struct ShellTool {
    /// Timeout for command execution.
    timeout: Duration,
    /// Working directory for commands.
    cwd: std::path::PathBuf,
    /// Policy for command approval.
    policy: Arc<dyn CommandPolicy>,
    /// Sandbox for command isolation.
    sandbox: Box<dyn Sandbox>,
}

impl ShellTool {
    /// Create a new shell tool with safe defaults.
    pub fn new(cwd: impl Into<std::path::PathBuf>) -> Self {
        Self {
            timeout: Duration::from_secs(120),
            cwd: cwd.into(),
            policy: Arc::new(SafePolicy::default()),
            sandbox: Box::new(NoSandbox),
        }
    }

    /// Set the timeout for commands.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set a custom command policy.
    pub fn with_policy(mut self, policy: Arc<dyn CommandPolicy>) -> Self {
        self.policy = policy;
        self
    }

    /// Set a sandbox for command isolation.
    pub fn with_sandbox(mut self, sandbox: Box<dyn Sandbox>) -> Self {
        self.sandbox = sandbox;
        self
    }
}

fn frontend_tool_cache_dir(cwd: &Path) -> PathBuf {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let cache_key = cwd
        .to_string_lossy()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let preferred = std::env::temp_dir()
        .join("octos-frontend-tool-cache")
        .join(user)
        .join(cache_key);
    let _ = std::fs::create_dir_all(&preferred);
    preferred
}

fn apply_frontend_tool_env(cmd: &mut tokio::process::Command, cwd: &Path) {
    let cache_dir = frontend_tool_cache_dir(cwd);
    cmd.env("ASTRO_TELEMETRY_DISABLED", "1")
        .env("NPM_CONFIG_CACHE", &cache_dir)
        .env("npm_config_cache", &cache_dir);
}

fn extract_git_invocation(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    if trimmed.starts_with("git ") {
        return Some(trimmed);
    }
    trimmed.find("git ").map(|idx| trimmed[idx..].trim())
}

fn is_git_repo_mismatch(output: &str) -> bool {
    let lowered = output.to_ascii_lowercase();
    lowered.contains("not a git repository")
        || lowered.contains("outside a working tree")
        || lowered.contains("use --no-index")
}

fn is_git_diff_command(command: &str) -> bool {
    extract_git_invocation(command)
        .map(|git_command| git_command.starts_with("git diff"))
        .unwrap_or(false)
}

fn extract_git_diff_targets(command: &str) -> Vec<String> {
    let Some(git_command) = extract_git_invocation(command) else {
        return Vec::new();
    };
    if !git_command.starts_with("git diff") {
        return Vec::new();
    }
    let Some((_, paths)) = git_command.split_once(" -- ") else {
        return Vec::new();
    };
    paths.split_whitespace().map(str::to_string).collect()
}

fn shell_quote_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    format!("'{}'", raw.replace('\'', "'\"'\"'"))
}

fn collect_git_repos(root: &Path, remaining_depth: usize, repos: &mut Vec<PathBuf>) {
    if remaining_depth == 0 {
        return;
    }

    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join(".git").is_dir() {
            repos.push(path.clone());
        }
        collect_git_repos(&path, remaining_depth - 1, repos);
    }
}

fn temp_search_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir(), PathBuf::from("/tmp")];
    roots.sort();
    roots.dedup();
    roots
}

fn select_best_git_repo_for_targets(targets: &[String]) -> Option<PathBuf> {
    if targets.is_empty() {
        return None;
    }

    let mut repos = Vec::new();
    for root in temp_search_roots() {
        collect_git_repos(&root, 2, &mut repos);
    }
    repos.sort();
    repos.dedup();

    repos
        .into_iter()
        .filter(|repo| targets.iter().all(|target| repo.join(target).exists()))
        .max_by_key(|repo| {
            targets
                .iter()
                .filter_map(|target| {
                    std::fs::metadata(repo.join(target))
                        .ok()
                        .and_then(|meta| meta.modified().ok())
                })
                .max()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
}

fn enrich_shell_failure_output(command: &str, cwd: &Path, output: &str) -> String {
    let Some(git_command) = extract_git_invocation(command) else {
        return output.to_string();
    };
    if !is_git_repo_mismatch(output) {
        return output.to_string();
    }

    let repo_hint = if is_git_diff_command(command) {
        select_best_git_repo_for_targets(&extract_git_diff_targets(command))
            .map(|repo| format!("Likely repo root: {}\n", repo.display()))
            .unwrap_or_default()
    } else {
        String::new()
    };

    format!(
        "{output}\n\nHint: This git command did not run inside a repository.\nCurrent shell cwd: {}\n{}Shell tool invocations are stateless, so `cd` in one shell call does not persist into the next.\nIf the repo lives elsewhere, rerun the git command in the same shell call as the directory change, for example:\ncd /path/to/repo && {git_command}",
        cwd.display(),
        repo_hint
    )
}

async fn execute_shell_command(
    sandbox: &dyn Sandbox,
    command: &str,
    cwd: &Path,
    timeout_duration: Duration,
) -> ToolResult {
    // Execute command (through sandbox).
    // Spawn the child, grab its PID, then timeout on wait_with_output().
    // If timeout fires, kill by PID to prevent orphaned processes.
    // (wait_with_output() takes ownership of child, so we save the PID first.)
    let mut cmd = sandbox.wrap_command(command, cwd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    apply_frontend_tool_env(&mut cmd, cwd);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                output: format!("Failed to execute command: {e}"),
                success: false,
                ..Default::default()
            };
        }
    };
    let child_pid = child.id();

    let result = timeout(timeout_duration, child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            let mut result_text = String::new();

            if !stdout.is_empty() {
                result_text.push_str(&stdout);
            }

            if !stderr.is_empty() {
                if !result_text.is_empty() {
                    result_text.push_str("\n--- stderr ---\n");
                }
                result_text.push_str(&stderr);
            }

            if result_text.is_empty() {
                result_text = "(no output)".to_string();
            }

            let exit_suffix = format!("\n\nExit code: {exit_code}");
            const MAX_OUTPUT: usize = 50000;
            octos_core::truncate_utf8(
                &mut result_text,
                MAX_OUTPUT - exit_suffix.len(),
                "\n... (output truncated)",
            );
            result_text.push_str(&exit_suffix);

            ToolResult {
                output: result_text,
                success: output.status.success(),
                ..Default::default()
            }
        }
        Ok(Err(e)) => ToolResult {
            output: format!("Failed to execute command: {e}"),
            success: false,
            ..Default::default()
        },
        Err(_) => {
            #[cfg(unix)]
            if let Some(pid) = child_pid {
                use std::process::Command as StdCommand;

                let _ = StdCommand::new("kill")
                    .args(["-15", &format!("-{pid}")])
                    .status();
                let _ = StdCommand::new("kill")
                    .args(["-15", &pid.to_string()])
                    .status();

                tokio::time::sleep(Duration::from_millis(500)).await;

                let still_alive = StdCommand::new("kill")
                    .args(["-0", &pid.to_string()])
                    .status()
                    .is_ok_and(|s| s.success());

                if still_alive {
                    let _ = StdCommand::new("kill")
                        .args(["-9", &format!("-{pid}")])
                        .status();
                    let _ = StdCommand::new("kill")
                        .args(["-9", &pid.to_string()])
                        .status();
                }
            }
            #[cfg(windows)]
            if let Some(pid) = child_pid {
                use std::process::Command as StdCommand;
                let _ = StdCommand::new("taskkill")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .status();
            }
            ToolResult {
                output: format!(
                    "Command timed out after {} seconds",
                    timeout_duration.as_secs()
                ),
                success: false,
                ..Default::default()
            }
        }
    }
}

async fn maybe_auto_recover_git_diff(
    sandbox: &dyn Sandbox,
    command: &str,
    output: &str,
    timeout_duration: Duration,
) -> Option<ToolResult> {
    if !is_git_diff_command(command) || !is_git_repo_mismatch(output) {
        return None;
    }

    let repo_root = select_best_git_repo_for_targets(&extract_git_diff_targets(command))?;
    let git_command = extract_git_invocation(command)?;
    let recovered_command = format!("cd {} && {}", shell_quote_path(&repo_root), git_command);
    let recovered = execute_shell_command(
        sandbox,
        &recovered_command,
        Path::new("/"),
        timeout_duration,
    )
    .await;

    if recovered.success
        && (recovered.output.contains("diff --git")
            || (recovered.output.contains("\n--- ") && recovered.output.contains("\n+++ ")))
    {
        Some(recovered)
    } else {
        None
    }
}

#[derive(Debug, Deserialize)]
struct ShellInput {
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return the output. Use this to run tests, build code, or interact with the filesystem."
    }

    fn tags(&self) -> &[&str] {
        &["runtime", "code"]
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Optional timeout in seconds (default: 120)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: ShellInput =
            serde_json::from_value(args.clone()).wrap_err("invalid shell tool input")?;

        // Check policy first
        let decision = self.policy.check(&input.command, &self.cwd);
        match decision {
            Decision::Deny => {
                tracing::warn!(command = %input.command, "command denied by policy");
                return Ok(ToolResult {
                    output: format!(
                        "Command denied by security policy: {}\n\nThis command was blocked because it matches a dangerous pattern.",
                        input.command
                    ),
                    success: false,
                    ..Default::default()
                });
            }
            Decision::Ask => {
                tracing::warn!(command = %input.command, "command requires approval — denied (no interactive approval available)");
                return Ok(ToolResult {
                    output: format!(
                        "Command requires approval and was denied: {}\n\nThis command matches a potentially dangerous pattern (e.g. sudo, rm -rf, git push --force). It cannot be executed without interactive approval.",
                        input.command
                    ),
                    success: false,
                    ..Default::default()
                });
            }
            Decision::Allow => {}
        }

        // Clamp timeout to [1, 600] seconds to prevent abuse
        const MIN_TIMEOUT: u64 = 1;
        const MAX_TIMEOUT: u64 = 600;
        let timeout_duration = input
            .timeout_secs
            .map(|s| Duration::from_secs(s.clamp(MIN_TIMEOUT, MAX_TIMEOUT)))
            .unwrap_or(self.timeout);

        let mut result = execute_shell_command(
            self.sandbox.as_ref(),
            &input.command,
            &self.cwd,
            timeout_duration,
        )
        .await;

        if !result.success {
            if let Some(recovered) = maybe_auto_recover_git_diff(
                self.sandbox.as_ref(),
                &input.command,
                &result.output,
                timeout_duration,
            )
            .await
            {
                return Ok(recovered);
            }

            result.output = enrich_shell_failure_output(&input.command, &self.cwd, &result.output);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_timeout_clamped_to_max() {
        let tool = ShellTool::new(std::env::temp_dir());
        let result = tool
            .execute(&serde_json::json!({
                "command": "echo hello",
                "timeout_secs": 999999
            }))
            .await
            .unwrap();
        // Should complete (clamped to 600s, not hang)
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_timeout_zero_clamped_to_min() {
        let tool = ShellTool::new(std::env::temp_dir());
        // timeout_secs: 0 would be clamped to 1 second
        let result = tool
            .execute(&serde_json::json!({
                "command": "echo fast",
                "timeout_secs": 0
            }))
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_denied_command() {
        let tool = ShellTool::new(std::env::temp_dir());
        let result = tool
            .execute(&serde_json::json!({"command": "rm -rf /"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.output.contains("denied"));
    }

    #[tokio::test]
    async fn test_ask_command_denied_without_approval() {
        let tool = ShellTool::new(std::env::temp_dir());
        // sudo triggers Ask, which must be denied (no interactive approval)
        let result = tool
            .execute(&serde_json::json!({"command": "sudo ls"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.output.contains("requires approval"));
    }

    #[tokio::test]
    async fn test_shell_sets_frontend_build_env() {
        let cwd = std::env::temp_dir().join(format!("octos-shell-env-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();

        let tool = ShellTool::new(&cwd);
        let result = tool
            .execute(&serde_json::json!({
                "command": "printf '%s\\n%s\\n' \"$ASTRO_TELEMETRY_DISABLED\" \"$NPM_CONFIG_CACHE\""
            }))
            .await
            .unwrap();

        assert!(result.success);
        let mut lines = result.output.lines();
        assert_eq!(lines.next(), Some("1"));
        let cache = lines.next().unwrap_or_default();
        assert!(cache.contains("octos-frontend-tool-cache"));
        assert!(!cache.contains(".octos-tool-cache"));
    }

    #[test]
    fn enrich_shell_failure_output_adds_git_repo_recovery_hint() {
        let cwd = std::path::Path::new("/tmp/octos-session");
        let output = "warning: Not a git repository. Use --no-index to compare two paths outside a working tree";

        let enriched = enrich_shell_failure_output("git diff -- notes.txt", cwd, output);

        assert!(enriched.contains("Current shell cwd: /tmp/octos-session"));
        assert!(enriched.contains("cd` in one shell call does not persist into the next"));
        assert!(enriched.contains("cd /path/to/repo && git diff -- notes.txt"));
    }

    #[test]
    fn enrich_shell_failure_output_ignores_non_git_errors() {
        let cwd = std::path::Path::new("/tmp/octos-session");
        let output = "cat: missing-file: No such file or directory";

        let enriched = enrich_shell_failure_output("cat missing-file", cwd, output);

        assert_eq!(enriched, output);
    }

    #[tokio::test]
    async fn auto_recovers_git_diff_from_recent_temp_repo() {
        let repo_root = std::env::temp_dir().join(format!(
            "octos-shell-recover-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&repo_root).unwrap();

        let setup = ShellTool::new(&repo_root);
        for command in [
            "git init",
            "printf 'alpha\\nbeta\\n' > notes.txt",
            "git add notes.txt",
            "git -c user.name=octos -c user.email=octos@example.com commit -m init",
            "printf 'alpha\\ngamma\\n' > notes.txt",
        ] {
            let result = setup
                .execute(&serde_json::json!({ "command": command }))
                .await
                .unwrap();
            assert!(result.success, "{command} failed: {}", result.output);
        }

        let tool = ShellTool::new(std::env::temp_dir());
        let result = tool
            .execute(&serde_json::json!({ "command": "git diff -- notes.txt" }))
            .await
            .unwrap();

        assert!(result.success, "auto-recovery failed: {}", result.output);
        assert!(result.output.contains("diff --git"));
        assert!(result.output.contains("-beta"));
        assert!(result.output.contains("+gamma"));

        let _ = std::fs::remove_dir_all(&repo_root);
    }
}
