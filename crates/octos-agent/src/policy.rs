//! Command approval policy.
//!
//! This module provides command approval before execution.
//! It's designed to be extended with codex-execpolicy when available.

use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Decision for a command execution request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Allow the command to execute.
    Allow,
    /// Deny the command.
    Deny,
    /// Ask the user for approval.
    Ask,
}

/// Policy for approving command execution.
pub trait CommandPolicy: Send + Sync {
    /// Check if a command should be allowed.
    fn check(&self, command: &str, cwd: &std::path::Path) -> Decision;
}

// ---------------------------------------------------------------------------
// Approval pipeline
// ---------------------------------------------------------------------------

/// Description of an operation that requires operator approval.
///
/// This is the payload handed to a [`PermissionApprover`] when a tool encounters
/// a [`Decision::Ask`] result. Keeping it a plain data type lets the same shape
/// flow through CLI prompts, HTTP approval cards, and (eventually) cross-process
/// mailbox messages without coupling the trait to any one transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    /// Tool that originated the request (e.g. `"shell"`).
    pub tool: String,
    /// Human-readable summary of the action (typically the command line).
    pub action: String,
    /// Why approval is required (e.g. matched policy pattern).
    pub reason: String,
}

/// The operator's response to a [`PermissionRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalResponse {
    /// Allow this single invocation.
    Allow,
    /// Allow this invocation and remember the approval for the rest of the
    /// session, so identical actions are auto-approved without re-prompting.
    AllowAlways,
    /// Reject this invocation.
    Deny,
}

/// Pluggable approver invoked when a [`CommandPolicy`] returns [`Decision::Ask`].
///
/// Implementations decide how to surface the request: a CLI prompt, a dashboard
/// approval card, an auto-allow for tests, etc. The trait is async so impls can
/// await user input or a remote response without blocking the agent loop.
#[async_trait]
pub trait PermissionApprover: Send + Sync {
    async fn approve(&self, request: PermissionRequest) -> ApprovalResponse;
}

/// Approver that denies every request. Backwards-compatible default that
/// preserves the previous "Ask = Deny" behavior when no operator is wired up.
pub struct DenyApprover;

#[async_trait]
impl PermissionApprover for DenyApprover {
    async fn approve(&self, _request: PermissionRequest) -> ApprovalResponse {
        ApprovalResponse::Deny
    }
}

/// Approver that allows every request. Intended for tests and `FULL_AUTO`-style
/// trusted environments — never wire this into an interactive session.
pub struct AllowAllApprover;

#[async_trait]
impl PermissionApprover for AllowAllApprover {
    async fn approve(&self, _request: PermissionRequest) -> ApprovalResponse {
        ApprovalResponse::Allow
    }
}

/// Wraps another approver with a session-scoped cache so that responses of
/// [`ApprovalResponse::AllowAlways`] short-circuit subsequent identical actions
/// without re-prompting the operator.
///
/// Cache keys are `(tool, action)` pairs. The cache lives for the lifetime of
/// the wrapper instance — typically one chat/session.
pub struct SessionCachedApprover<A: PermissionApprover> {
    inner: A,
    remembered: Mutex<std::collections::HashSet<(String, String)>>,
}

impl<A: PermissionApprover> SessionCachedApprover<A> {
    pub fn new(inner: A) -> Self {
        Self {
            inner,
            remembered: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Number of remembered (tool, action) entries. Test-only.
    #[cfg(test)]
    fn remembered_count(&self) -> usize {
        self.remembered.lock().unwrap().len()
    }
}

#[async_trait]
impl<A: PermissionApprover> PermissionApprover for SessionCachedApprover<A> {
    async fn approve(&self, request: PermissionRequest) -> ApprovalResponse {
        let key = (request.tool.clone(), request.action.clone());
        if self.remembered.lock().unwrap().contains(&key) {
            return ApprovalResponse::Allow;
        }
        let response = self.inner.approve(request).await;
        if matches!(response, ApprovalResponse::AllowAlways) {
            self.remembered.lock().unwrap().insert(key);
        }
        response
    }
}

/// Default policy that allows all commands.
/// Use this for trusted environments.
pub struct AllowAllPolicy;

impl CommandPolicy for AllowAllPolicy {
    fn check(&self, _command: &str, _cwd: &std::path::Path) -> Decision {
        Decision::Allow
    }
}

/// Policy that denies a small set of obviously dangerous commands.
///
/// **Not a security boundary.** `SafePolicy` catches common accidents (e.g.,
/// `rm -rf /`, fork bombs) via simple pattern matching on whitespace-normalized
/// command strings. It is trivially bypassable — shell metacharacters, variable
/// expansion (`rm${IFS}-rf${IFS}/`), encoding tricks, and any command not on the
/// short deny list all pass through unblocked.
///
/// Real isolation must come from the sandbox layer ([`super::sandbox`]). Treat
/// `SafePolicy` as defense-in-depth for obvious mistakes, not as a guarantee
/// that dangerous commands cannot execute.
pub struct SafePolicy {
    /// Patterns that should be denied.
    deny_patterns: Vec<String>,
    /// Patterns that should always ask.
    ask_patterns: Vec<String>,
}

impl Default for SafePolicy {
    fn default() -> Self {
        Self {
            deny_patterns: vec![
                "rm -rf /".to_string(),
                "rm -rf /*".to_string(),
                "dd if=".to_string(),
                "mkfs".to_string(),
                ":(){:|:&};:".to_string(), // Fork bomb
                "chmod -R 777 /".to_string(),
            ],
            ask_patterns: vec![
                "sudo".to_string(),
                "rm -rf".to_string(),
                "git push --force".to_string(),
                "git reset --hard".to_string(),
            ],
        }
    }
}

/// Collapse consecutive whitespace into single spaces and trim.
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Check if `pattern` appears in `haystack` at a word boundary.
///
/// A word boundary is start/end of string or a non-alphanumeric character.
/// This prevents "mkfs" from matching inside "unmkfsblah" or "sudo" inside "pseudocode".
fn contains_at_word_boundary(haystack: &str, pattern: &str) -> bool {
    let pat_bytes = pattern.as_bytes();
    let hay_bytes = haystack.as_bytes();
    if pat_bytes.len() > hay_bytes.len() {
        return false;
    }
    for i in 0..=(hay_bytes.len() - pat_bytes.len()) {
        if &hay_bytes[i..i + pat_bytes.len()] == pat_bytes {
            // Check left boundary: start of string or non-alphanumeric
            let left_ok = i == 0 || !hay_bytes[i - 1].is_ascii_alphanumeric();
            // Check right boundary: end of string or non-alphanumeric
            let right_ok = i + pat_bytes.len() == hay_bytes.len()
                || !hay_bytes[i + pat_bytes.len()].is_ascii_alphanumeric();
            if left_ok && right_ok {
                return true;
            }
        }
    }
    false
}

impl CommandPolicy for SafePolicy {
    fn check(&self, command: &str, _cwd: &std::path::Path) -> Decision {
        let normalized = normalize_whitespace(command);

        // Check deny patterns first
        for pattern in &self.deny_patterns {
            if contains_at_word_boundary(&normalized, pattern) {
                return Decision::Deny;
            }
        }

        // Check ask patterns
        for pattern in &self.ask_patterns {
            if contains_at_word_boundary(&normalized, pattern) {
                return Decision::Ask;
            }
        }

        Decision::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_allow_all_policy() {
        let policy = AllowAllPolicy;
        assert_eq!(policy.check("rm -rf /", Path::new("/")), Decision::Allow);
    }

    #[test]
    fn test_safe_policy_deny() {
        let policy = SafePolicy::default();
        assert_eq!(policy.check("rm -rf /", Path::new("/tmp")), Decision::Deny);
        assert_eq!(
            policy.check("dd if=/dev/zero of=/dev/sda", Path::new("/tmp")),
            Decision::Deny
        );
    }

    #[test]
    fn test_safe_policy_ask() {
        let policy = SafePolicy::default();
        assert_eq!(
            policy.check("sudo apt install foo", Path::new("/tmp")),
            Decision::Ask
        );
        assert_eq!(
            policy.check("git push --force origin main", Path::new("/tmp")),
            Decision::Ask
        );
    }

    #[test]
    fn test_safe_policy_whitespace_bypass() {
        let policy = SafePolicy::default();
        // Double-space and tab variants must still be caught
        assert_eq!(
            policy.check("rm  -rf  /", Path::new("/tmp")),
            Decision::Deny
        );
        assert_eq!(
            policy.check("rm\t-rf\t/", Path::new("/tmp")),
            Decision::Deny
        );
        assert_eq!(
            policy.check("git  push  --force origin main", Path::new("/tmp")),
            Decision::Ask
        );
    }

    #[test]
    fn test_safe_policy_allow() {
        let policy = SafePolicy::default();
        assert_eq!(
            policy.check("cargo build", Path::new("/tmp")),
            Decision::Allow
        );
        assert_eq!(
            policy.check("git status", Path::new("/tmp")),
            Decision::Allow
        );
    }

    #[test]
    fn test_safe_policy_word_boundary() {
        let policy = SafePolicy::default();
        // "sudo" should NOT match inside "pseudocode"
        assert_eq!(
            policy.check("pseudocode is fun", Path::new("/tmp")),
            Decision::Allow
        );
        // "mkfs" should NOT match inside "unmkfs"
        assert_eq!(
            policy.check("unmkfs something", Path::new("/tmp")),
            Decision::Allow
        );
        // But standalone "mkfs" should still be caught
        assert_eq!(
            policy.check("mkfs /dev/sda", Path::new("/tmp")),
            Decision::Deny
        );
        // And "sudo" standalone should still be caught
        assert_eq!(policy.check("sudo ls", Path::new("/tmp")), Decision::Ask);
        // Pattern at end of string
        assert_eq!(policy.check("run sudo", Path::new("/tmp")), Decision::Ask);
    }

    // -----------------------------------------------------------------------
    // PermissionApprover pipeline
    // -----------------------------------------------------------------------

    fn req(action: &str) -> PermissionRequest {
        PermissionRequest {
            tool: "shell".to_string(),
            action: action.to_string(),
            reason: "matched ask pattern".to_string(),
        }
    }

    #[tokio::test]
    async fn deny_approver_always_denies() {
        let approver = DenyApprover;
        assert_eq!(
            approver.approve(req("sudo ls")).await,
            ApprovalResponse::Deny,
        );
    }

    #[tokio::test]
    async fn allow_all_approver_always_allows() {
        let approver = AllowAllApprover;
        assert_eq!(
            approver.approve(req("sudo ls")).await,
            ApprovalResponse::Allow,
        );
    }

    /// Approver that returns scripted responses in FIFO order so we can simulate
    /// an operator answering AllowAlways then closing the session.
    struct ScriptedApprover {
        responses: Mutex<Vec<ApprovalResponse>>,
        seen: Mutex<Vec<PermissionRequest>>,
    }

    impl ScriptedApprover {
        fn new(responses: Vec<ApprovalResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl PermissionApprover for ScriptedApprover {
        async fn approve(&self, request: PermissionRequest) -> ApprovalResponse {
            self.seen.lock().unwrap().push(request);
            self.responses
                .lock()
                .unwrap()
                .pop()
                .expect("ScriptedApprover: no more scripted responses")
        }
    }

    #[tokio::test]
    async fn session_cache_short_circuits_after_allow_always() {
        // Script: first call → AllowAlways, second call would panic if reached.
        let scripted = ScriptedApprover::new(vec![ApprovalResponse::AllowAlways]);
        let approver = SessionCachedApprover::new(scripted);

        // First call: hits inner, gets AllowAlways, cached.
        let r1 = approver.approve(req("sudo ls")).await;
        assert_eq!(r1, ApprovalResponse::AllowAlways);
        assert_eq!(approver.remembered_count(), 1);

        // Second call with the same (tool, action): bypasses inner entirely
        // (otherwise the empty script vec would panic) and resolves to Allow.
        let r2 = approver.approve(req("sudo ls")).await;
        assert_eq!(r2, ApprovalResponse::Allow);
    }

    #[tokio::test]
    async fn session_cache_does_not_remember_one_shot_allow() {
        // Script returns Allow twice — both calls must reach the inner approver.
        let scripted =
            ScriptedApprover::new(vec![ApprovalResponse::Allow, ApprovalResponse::Allow]);
        let approver = SessionCachedApprover::new(scripted);

        approver.approve(req("sudo ls")).await;
        approver.approve(req("sudo ls")).await;

        assert_eq!(approver.remembered_count(), 0);
    }

    #[tokio::test]
    async fn session_cache_keys_on_action_not_just_tool() {
        // AllowAlways for `sudo ls` must NOT auto-approve `sudo rm -rf /tmp/foo`.
        let scripted =
            ScriptedApprover::new(vec![ApprovalResponse::Deny, ApprovalResponse::AllowAlways]);
        let approver = SessionCachedApprover::new(scripted);

        // First action: AllowAlways (popped from end of vec).
        assert_eq!(
            approver.approve(req("sudo ls")).await,
            ApprovalResponse::AllowAlways,
        );
        // Second, different action: must hit inner approver, gets Deny.
        assert_eq!(
            approver.approve(req("sudo rm -rf /tmp/foo")).await,
            ApprovalResponse::Deny,
        );
    }
}
