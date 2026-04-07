//! Git worktree isolation for swarm sub-agents.
//!
//! Each spawned worker can be allocated its own git worktree under
//! `.octos/work/<slug>/` so that concurrent edits do not race against the
//! parent or against sibling workers. The worker's `working_dir` is then set
//! to the worktree path; the rest of the agent stack (ToolRegistry,
//! file tools, sandbox path rules) inherits the new directory transparently.
//!
//! Slug validation is ported from HKUDS/OpenHarness's `swarm/worktree.py` and
//! is the only path-traversal defense between user/LLM input and `git
//! worktree add`. Treat it as a security boundary.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, eyre};
use tokio::process::Command;

const MAX_SLUG_LENGTH: usize = 64;

/// Sanitize and validate a worktree slug.
///
/// Rules (matching OpenHarness):
/// - Non-empty, ≤ 64 characters total
/// - Each `/`-separated segment must match `[a-zA-Z0-9._-]+`
/// - `.` and `..` segments are rejected (path traversal)
/// - Leading `/` or `\` are rejected (absolute paths)
///
/// Returns the slug unchanged if valid, [`Err`] otherwise.
pub fn validate_worktree_slug(slug: &str) -> Result<&str> {
    if slug.is_empty() {
        return Err(eyre!("worktree slug must not be empty"));
    }
    if slug.len() > MAX_SLUG_LENGTH {
        return Err(eyre!(
            "worktree slug must be {MAX_SLUG_LENGTH} characters or fewer (got {})",
            slug.len()
        ));
    }
    if slug.starts_with('/') || slug.starts_with('\\') {
        return Err(eyre!("worktree slug must not be an absolute path: {slug}"));
    }

    for segment in slug.split('/') {
        if segment == "." || segment == ".." {
            return Err(eyre!(
                "worktree slug {slug:?}: must not contain \".\" or \"..\" path segments"
            ));
        }
        if segment.is_empty() {
            return Err(eyre!(
                "worktree slug {slug:?}: must not contain empty segments"
            ));
        }
        if !segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(eyre!(
                "worktree slug {slug:?}: each segment must contain only \
                 letters, digits, dots, underscores, and dashes"
            ));
        }
    }

    Ok(slug)
}

/// Replace `/` with `+` so a multi-segment slug becomes a single directory
/// name and a single git branch name. Mirrors OpenHarness's `_flatten_slug`.
pub fn flatten_slug(slug: &str) -> String {
    slug.replace('/', "+")
}

/// Metadata about a managed git worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Original slug as supplied by the caller (post-validation).
    pub slug: String,
    /// Absolute path to the allocated worktree.
    pub path: PathBuf,
    /// Branch name created for the worktree.
    pub branch: String,
    /// The parent repository path the worktree was carved from.
    pub original_path: PathBuf,
}

/// Allocate a new git worktree for a sub-agent.
///
/// Creates `<parent>/.octos/work/<flattened_slug>/` checked out on a fresh
/// branch `octos/worker/<flattened_slug>` rooted at the parent's `HEAD`.
///
/// Returns [`Err`] if the slug is invalid, the parent is not a git repo, or
/// the underlying `git worktree add` invocation fails (most commonly: the
/// destination directory already exists, or the branch name collides).
pub async fn allocate_worktree(parent: &Path, slug: &str) -> Result<WorktreeInfo> {
    let validated = validate_worktree_slug(slug)?.to_string();
    let flat = flatten_slug(&validated);
    let path = parent.join(".octos").join("work").join(&flat);
    let branch = format!("octos/worker/{flat}");

    // git worktree add will create intermediate dirs *under* the target, but
    // it does not create the parent containing dir. Make sure `.octos/work/`
    // exists so the call doesn't fail with ENOENT on a fresh repo.
    if let Some(parent_dir) = path.parent() {
        tokio::fs::create_dir_all(parent_dir)
            .await
            .wrap_err_with(|| {
                format!(
                    "creating worktree parent directory {}",
                    parent_dir.display()
                )
            })?;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(parent)
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(&branch)
        .arg(&path)
        .output()
        .await
        .wrap_err("invoking git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!(
            "git worktree add failed for slug {slug:?}: {}",
            stderr.trim()
        ));
    }

    Ok(WorktreeInfo {
        slug: validated,
        path,
        branch,
        original_path: parent.to_path_buf(),
    })
}

/// Tear down a previously allocated worktree.
///
/// Runs `git worktree remove --force <path>` against the original repository.
/// The branch created in [`allocate_worktree`] is left in place so the user
/// can inspect or merge it after the fact; pruning branches is the user's job.
pub async fn cleanup_worktree(info: &WorktreeInfo) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(&info.original_path)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(&info.path)
        .output()
        .await
        .wrap_err("invoking git worktree remove")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("git worktree remove failed: {}", stderr.trim()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- slug validation -----

    #[test]
    fn rejects_empty_slug() {
        assert!(validate_worktree_slug("").is_err());
    }

    #[test]
    fn rejects_slug_over_64_chars() {
        let long = "a".repeat(65);
        assert!(validate_worktree_slug(&long).is_err());
    }

    #[test]
    fn accepts_slug_at_64_chars() {
        let on_limit = "a".repeat(64);
        assert!(validate_worktree_slug(&on_limit).is_ok());
    }

    #[test]
    fn rejects_absolute_paths() {
        assert!(validate_worktree_slug("/etc/passwd").is_err());
        assert!(validate_worktree_slug("\\windows\\system").is_err());
    }

    #[test]
    fn rejects_dot_traversal() {
        assert!(validate_worktree_slug("..").is_err());
        assert!(validate_worktree_slug("../../secrets").is_err());
        assert!(validate_worktree_slug("foo/../bar").is_err());
        assert!(validate_worktree_slug("./foo").is_err());
    }

    #[test]
    fn rejects_invalid_characters() {
        assert!(validate_worktree_slug("worker space").is_err());
        assert!(validate_worktree_slug("worker;ls").is_err());
        assert!(validate_worktree_slug("worker$(echo)").is_err());
        assert!(validate_worktree_slug("worker\0bad").is_err());
    }

    #[test]
    fn rejects_empty_segments() {
        assert!(validate_worktree_slug("foo//bar").is_err());
    }

    #[test]
    fn accepts_typical_agent_ids() {
        assert!(validate_worktree_slug("subagent-0").is_ok());
        assert!(validate_worktree_slug("subagent-42").is_ok());
        assert!(validate_worktree_slug("review.bot_1").is_ok());
        assert!(validate_worktree_slug("group/worker-1").is_ok());
    }

    #[test]
    fn flatten_replaces_slashes_with_plus() {
        assert_eq!(flatten_slug("group/worker-1"), "group+worker-1");
        assert_eq!(flatten_slug("subagent-0"), "subagent-0");
    }

    // ----- allocate / cleanup -----
    //
    // These tests shell out to `git`, so they're gated on the binary being
    // available; CI without git installed should still pass.

    async fn git_init(dir: &Path) -> Result<()> {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .output()
            .await?;
        // Worktrees require at least one commit so HEAD resolves.
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .args(["commit", "--allow-empty", "-q", "-m", "init"])
            .output()
            .await?;
        Ok(())
    }

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn allocate_then_cleanup_roundtrip() {
        if !git_available() {
            eprintln!("git not available — skipping");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        git_init(tmp.path()).await.unwrap();

        let info = allocate_worktree(tmp.path(), "test-worker").await.unwrap();
        assert!(info.path.exists(), "worktree path should exist");
        assert!(
            info.path.join(".git").exists(),
            "worktree should be a git checkout (has .git file)"
        );
        assert_eq!(info.branch, "octos/worker/test-worker");

        cleanup_worktree(&info).await.unwrap();
        assert!(
            !info.path.exists(),
            "worktree path should be removed after cleanup"
        );
    }

    #[tokio::test]
    async fn allocate_in_non_git_dir_fails() {
        if !git_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        // No `git init` — parent isn't a repo.
        let result = allocate_worktree(tmp.path(), "test-worker").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn allocate_rejects_invalid_slug_before_invoking_git() {
        // No git_available check needed — validation runs first.
        let tmp = tempfile::tempdir().unwrap();
        let result = allocate_worktree(tmp.path(), "../bad").await;
        assert!(result.is_err());
        assert!(
            !tmp.path().join(".octos").exists(),
            "no fs side effects on validation failure"
        );
    }

    #[tokio::test]
    async fn allocate_two_siblings_in_same_repo() {
        if !git_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        git_init(tmp.path()).await.unwrap();

        let a = allocate_worktree(tmp.path(), "worker-a").await.unwrap();
        let b = allocate_worktree(tmp.path(), "worker-b").await.unwrap();

        assert!(a.path.exists());
        assert!(b.path.exists());
        assert_ne!(a.path, b.path);
        assert_ne!(a.branch, b.branch);

        cleanup_worktree(&a).await.unwrap();
        cleanup_worktree(&b).await.unwrap();
    }
}
