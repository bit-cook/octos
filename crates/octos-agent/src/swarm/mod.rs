//! Swarm subsystem: multi-agent coordination primitives.
//!
//! This module hosts the building blocks shared by spawned sub-agent workers:
//! per-worker filesystem isolation via git worktrees, persistent message
//! mailboxes, and (eventually) cross-process leader/worker coordination.
//!
//! Inspired by HKUDS/OpenHarness's `swarm/` package, ported to Rust with the
//! repo-native storage choices (redb for the mailbox, gix-friendly worktree
//! management) wherever the original would have used JSONL files or Python
//! shell-outs.

pub mod worktree;
