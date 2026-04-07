//! Pluggable async mailbox for swarm sub-agents.
//!
//! Two backends ship in-tree:
//!
//! 1. [`InProcessMailbox`] — a `tokio::sync::Mutex` over per-recipient
//!    `VecDeque`s. Fast, zero-config, dies with the parent process. This
//!    matches the historic `BackgroundResultSender` behavior used by
//!    [`crate::tools::spawn::SpawnTool`] and is the default for `octos chat`.
//!
//! 2. [`RedbMailbox`] — a redb-backed mailbox at `<dir>/mailbox.redb`. Crash-
//!    resilient, inspectable on disk, lets a `octos serve` restart resume
//!    pending messages without losing in-flight worker results. Built on the
//!    same redb dependency `octos-memory` already uses for `EpisodeStore`,
//!    so there is no new storage backend on the dependency graph.
//!
//! The trait surface is intentionally tiny — `send`, `recv`, `ack`, `list` —
//! so future backends (e.g. an HTTP-fronted shared mailbox) only need to
//! implement four methods.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use eyre::{Result, WrapErr};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Process-local monotonic counter used by [`RedbMailbox`] to break ties when
/// two messages share the same UUID v7 millisecond timestamp. UUID v7 only
/// encodes 48 bits of ms in its leading bits — the rest is random — so the
/// raw UUID alone does not preserve insertion order inside a millisecond.
/// Appending this counter to the redb composite key restores strict FIFO on
/// the storage side without changing `MailboxMessage.id` (which remains a
/// standard UUID v7 for cross-process reference and repo convention).
static MAILBOX_SEND_SEQ: AtomicU64 = AtomicU64::new(0);

/// Type tag for messages flowing through the mailbox. Mirrors OpenHarness's
/// MessageType enum so the wire format stays compatible if we ever bridge
/// the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxMessageType {
    /// Free-form text from the leader to a worker (or vice versa).
    UserMessage,
    /// Tool call requesting operator approval (paired with the #293
    /// PermissionApprover machinery; the cross-process glue lands later).
    PermissionRequest,
    /// Operator's response to a `PermissionRequest`.
    PermissionResponse,
    /// Worker has finished its turn and has nothing else to do — coordinator
    /// can reassign or shut it down. Used by #297.
    IdleNotification,
}

/// One message exchanged between swarm agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMessage {
    /// Globally unique ID. Lexicographic order matches creation order.
    pub id: String,
    pub kind: MailboxMessageType,
    pub sender: String,
    pub recipient: String,
    /// Free-form payload, opaque to the mailbox itself.
    pub payload: serde_json::Value,
    /// Wall-clock UNIX millis at the moment of `send`.
    pub timestamp_ms: u64,
}

impl MailboxMessage {
    /// Convenience constructor that fills in `id` and `timestamp_ms`.
    ///
    /// IDs are UUID v7 strings so their lexicographic order matches
    /// creation time — `MailboxBackend` implementations rely on this to
    /// deliver messages in send order via prefix range scans. UUID v7 is
    /// the repo's standard temporally-sortable ID (see `octos_core::TaskId`).
    pub fn new(
        kind: MailboxMessageType,
        sender: impl Into<String>,
        recipient: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            id: Uuid::now_v7().to_string(),
            kind,
            sender: sender.into(),
            recipient: recipient.into(),
            payload,
            timestamp_ms: now_ms,
        }
    }
}

/// Pluggable mailbox transport. Implementations must be cheap to clone and
/// safe to share across tasks (`Send + Sync`).
#[async_trait]
pub trait MailboxBackend: Send + Sync {
    /// Enqueue a message for `recipient`.
    async fn send(&self, message: MailboxMessage) -> Result<()>;

    /// Pop the oldest pending message addressed to `recipient`, if any.
    /// Returns `Ok(None)` when the inbox is empty (does NOT block).
    async fn recv(&self, recipient: &str) -> Result<Option<MailboxMessage>>;

    /// Mark a message as fully handled. The message MAY be deleted at this
    /// point; `recv` is allowed to delete on read instead, in which case
    /// `ack` is a no-op. Implementations must not error if the id is unknown.
    async fn ack(&self, message_id: &str) -> Result<()>;

    /// Snapshot the recipient's pending queue without consuming it. Used for
    /// resume-on-restart UI ("you have N undelivered messages").
    async fn list(&self, recipient: &str) -> Result<Vec<MailboxMessage>>;
}

/// Shared recipient-name validator used by every [`MailboxBackend`] impl.
///
/// Rejects empty names and any name containing `/`. The `/` restriction comes
/// from [`RedbMailbox`]'s composite-key scheme (`<recipient>/<id>/<seq>`), but
/// we enforce it on the in-process backend too so mixed-backend tests and
/// migrations behave identically.
fn validate_recipient(recipient: &str) -> Result<()> {
    if recipient.is_empty() {
        return Err(eyre::eyre!("mailbox recipient must not be empty"));
    }
    if recipient.contains('/') {
        return Err(eyre::eyre!(
            "mailbox recipient must not contain '/' (would collide with key prefix scheme): {recipient:?}"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// In-process backend
// ---------------------------------------------------------------------------

/// Lock-free-ish in-process mailbox. Backed by per-recipient `VecDeque`s
/// behind a single `Mutex`; perfectly fine for the small number of swarm
/// agents per session and avoids the overhead of building one channel per
/// recipient up front.
#[derive(Default, Clone)]
pub struct InProcessMailbox {
    inner: Arc<Mutex<HashMap<String, VecDeque<MailboxMessage>>>>,
}

impl InProcessMailbox {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl MailboxBackend for InProcessMailbox {
    async fn send(&self, message: MailboxMessage) -> Result<()> {
        validate_recipient(&message.recipient)?;
        let mut guard = self.inner.lock().await;
        guard
            .entry(message.recipient.clone())
            .or_default()
            .push_back(message);
        Ok(())
    }

    async fn recv(&self, recipient: &str) -> Result<Option<MailboxMessage>> {
        validate_recipient(recipient)?;
        let mut guard = self.inner.lock().await;
        Ok(guard.get_mut(recipient).and_then(|q| q.pop_front()))
    }

    async fn ack(&self, _message_id: &str) -> Result<()> {
        // recv() already removes the message. No-op.
        Ok(())
    }

    async fn list(&self, recipient: &str) -> Result<Vec<MailboxMessage>> {
        validate_recipient(recipient)?;
        let guard = self.inner.lock().await;
        Ok(guard
            .get(recipient)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// redb-backed backend
// ---------------------------------------------------------------------------

/// Schema:
///
/// - `mailbox`: composite key `<recipient>/<message_id>/<seq>` → JSON-encoded
///   `MailboxMessage`. Iterating with a `<recipient>/` prefix yields all
///   pending messages for that recipient in lexicographic (= insertion)
///   order. The trailing `seq` is a process-local monotonic counter that
///   breaks ties when two messages share the same UUID v7 millisecond; it
///   exists only in the redb key, not in `MailboxMessage.id`. Recipient
///   names containing `/` are rejected by [`validate_recipient`] on both
///   backends to avoid prefix collisions.
const MAILBOX_TABLE: TableDefinition<&str, &str> = TableDefinition::new("mailbox");

/// redb-backed mailbox. Persists at `<dir>/mailbox.redb`. Survives parent
/// process crashes; resume by calling [`MailboxBackend::list`] for any
/// recipient on startup.
pub struct RedbMailbox {
    db: Arc<Database>,
}

impl RedbMailbox {
    /// Open or create a redb mailbox in `dir`. The directory is created if
    /// missing. The on-disk file is `dir/mailbox.redb`.
    pub async fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&dir)
            .await
            .wrap_err("creating mailbox directory")?;
        let db_path = dir.join("mailbox.redb");
        let db = tokio::task::spawn_blocking(move || -> Result<Database> {
            let db = Database::create(&db_path).wrap_err("opening mailbox redb")?;
            // Initialize the table so empty-mailbox `list` calls don't fail.
            let write_txn = db.begin_write()?;
            {
                let _ = write_txn.open_table(MAILBOX_TABLE)?;
            }
            write_txn.commit()?;
            Ok(db)
        })
        .await??;
        Ok(Self { db: Arc::new(db) })
    }

    /// Build the redb composite key for a message.
    ///
    /// Format: `<recipient>/<message_id>/<seq:016x>`
    ///
    /// The trailing `seq` is a process-local monotonic counter fetched on
    /// every send. It guarantees that two messages inserted in the same
    /// UUID v7 millisecond still sort in send order — otherwise the random
    /// trailing bits of UUID v7 would reorder bursts. See
    /// [`MAILBOX_SEND_SEQ`] for the rationale.
    fn make_key(recipient: &str, message_id: &str) -> String {
        let seq = MAILBOX_SEND_SEQ.fetch_add(1, Ordering::Relaxed);
        format!("{recipient}/{message_id}/{seq:016x}")
    }
}

#[async_trait]
impl MailboxBackend for RedbMailbox {
    async fn send(&self, message: MailboxMessage) -> Result<()> {
        validate_recipient(&message.recipient)?;
        let db = self.db.clone();
        let json = serde_json::to_string(&message).wrap_err("serializing mailbox message")?;
        let key = Self::make_key(&message.recipient, &message.id);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(MAILBOX_TABLE)?;
                table.insert(key.as_str(), json.as_str())?;
            }
            write_txn.commit()?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn recv(&self, recipient: &str) -> Result<Option<MailboxMessage>> {
        validate_recipient(recipient)?;
        let db = self.db.clone();
        let prefix = format!("{recipient}/");
        tokio::task::spawn_blocking(move || -> Result<Option<MailboxMessage>> {
            // Single write transaction: read the oldest message for this
            // recipient, delete it, commit. Atomic with respect to other
            // writers and half the spawn_blocking round-trips of a
            // read-then-write split.
            let write_txn = db.begin_write()?;
            let found: Option<(String, MailboxMessage)> = {
                let table = write_txn.open_table(MAILBOX_TABLE)?;
                // Unbounded upper range + prefix check avoids the previous
                // `prefix..prefix+\u{FFFD}` trick, which was only correct as
                // long as keys stayed ASCII-only.
                let mut iter = table.range(prefix.as_str()..)?;
                match iter.next() {
                    Some(entry) => {
                        let (k, v) = entry?;
                        if k.value().starts_with(&prefix) {
                            let key = k.value().to_string();
                            let msg: MailboxMessage = serde_json::from_str(v.value())
                                .wrap_err("deserializing mailbox message")?;
                            Some((key, msg))
                        } else {
                            None
                        }
                    }
                    None => None,
                }
            };
            let msg = if let Some((key, msg)) = found {
                let mut table = write_txn.open_table(MAILBOX_TABLE)?;
                table.remove(key.as_str())?;
                Some(msg)
            } else {
                None
            };
            write_txn.commit()?;
            Ok(msg)
        })
        .await?
    }

    async fn ack(&self, _message_id: &str) -> Result<()> {
        // recv() deletes on read. ack is a no-op for symmetry with future
        // backends that may want explicit two-phase delivery.
        Ok(())
    }

    async fn list(&self, recipient: &str) -> Result<Vec<MailboxMessage>> {
        validate_recipient(recipient)?;
        let db = self.db.clone();
        let prefix = format!("{recipient}/");
        let messages = tokio::task::spawn_blocking(move || -> Result<Vec<MailboxMessage>> {
            let read_txn = db.begin_read()?;
            let table = read_txn.open_table(MAILBOX_TABLE)?;
            let mut out = Vec::new();
            for entry in table.range(prefix.as_str()..)? {
                let (key, value) = entry?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                let msg: MailboxMessage = serde_json::from_str(value.value())
                    .wrap_err("deserializing mailbox message")?;
                out.push(msg);
            }
            Ok(out)
        })
        .await??;
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(recipient: &str, payload: &str) -> MailboxMessage {
        MailboxMessage::new(
            MailboxMessageType::UserMessage,
            "leader",
            recipient,
            serde_json::json!({ "text": payload }),
        )
    }

    // ----- shared backend conformance -----

    async fn assert_backend_conformance(backend: Arc<dyn MailboxBackend>) {
        // empty inbox
        assert!(backend.recv("worker").await.unwrap().is_none());
        assert!(backend.list("worker").await.unwrap().is_empty());

        // FIFO ordering for the same recipient
        let m1 = msg("worker", "hello");
        let m2 = msg("worker", "world");
        backend.send(m1.clone()).await.unwrap();
        // Sleep 2ms so the timestamp-derived ID of m3 sorts after m1 even
        // when system clock granularity is coarse.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        backend.send(m2.clone()).await.unwrap();

        // list does not consume
        assert_eq!(backend.list("worker").await.unwrap().len(), 2);
        assert_eq!(backend.list("worker").await.unwrap().len(), 2);

        let r1 = backend.recv("worker").await.unwrap().unwrap();
        assert_eq!(r1.payload["text"], "hello");
        let r2 = backend.recv("worker").await.unwrap().unwrap();
        assert_eq!(r2.payload["text"], "world");

        // Inbox is now empty.
        assert!(backend.recv("worker").await.unwrap().is_none());

        // Different recipients are isolated.
        backend.send(msg("alice", "for-alice")).await.unwrap();
        backend.send(msg("bob", "for-bob")).await.unwrap();
        let alice = backend.recv("alice").await.unwrap().unwrap();
        assert_eq!(alice.payload["text"], "for-alice");
        // bob's queue is untouched.
        let bob = backend.recv("bob").await.unwrap().unwrap();
        assert_eq!(bob.payload["text"], "for-bob");

        // ack is a no-op but must not error.
        backend.ack("anything").await.unwrap();
    }

    // ----- in-process backend -----

    #[tokio::test]
    async fn in_process_backend_conformance() {
        let mb: Arc<dyn MailboxBackend> = Arc::new(InProcessMailbox::new());
        assert_backend_conformance(mb).await;
    }

    // ----- redb backend -----

    #[tokio::test]
    async fn redb_backend_conformance() {
        let dir = tempfile::tempdir().unwrap();
        let mb: Arc<dyn MailboxBackend> = Arc::new(RedbMailbox::open(dir.path()).await.unwrap());
        assert_backend_conformance(mb).await;
    }

    #[tokio::test]
    async fn redb_backend_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mb = RedbMailbox::open(dir.path()).await.unwrap();
            mb.send(msg("worker", "from-first-process")).await.unwrap();
        }
        // Drop the first instance, reopen.
        let mb = RedbMailbox::open(dir.path()).await.unwrap();
        let pending = mb.list("worker").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].payload["text"], "from-first-process");

        let recv = mb.recv("worker").await.unwrap().unwrap();
        assert_eq!(recv.payload["text"], "from-first-process");
        assert!(mb.recv("worker").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn redb_backend_rejects_recipient_with_slash() {
        let dir = tempfile::tempdir().unwrap();
        let mb = RedbMailbox::open(dir.path()).await.unwrap();
        let m = msg("a/b", "boom");
        let err = mb.send(m).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn message_ids_sort_chronologically() {
        // UUID v7 encodes a 48-bit ms timestamp in the leading bits, so
        // IDs created at least 1ms apart sort chronologically when
        // compared as strings.
        let m1 = MailboxMessage::new(
            MailboxMessageType::UserMessage,
            "s",
            "r",
            serde_json::json!({}),
        );
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let m2 = MailboxMessage::new(
            MailboxMessageType::UserMessage,
            "s",
            "r",
            serde_json::json!({}),
        );
        assert!(m1.id < m2.id, "{:?} should sort before {:?}", m1.id, m2.id);
    }

    #[tokio::test]
    async fn redb_recipient_isolation_under_prefix_scheme() {
        // Regression: a recipient name that is a prefix of another (`a` and
        // `aa`) must not bleed messages between inboxes. The `recipient/`
        // separator in the key prevents this.
        let dir = tempfile::tempdir().unwrap();
        let mb = RedbMailbox::open(dir.path()).await.unwrap();
        mb.send(msg("a", "for-a")).await.unwrap();
        mb.send(msg("aa", "for-aa")).await.unwrap();

        let a = mb.recv("a").await.unwrap().unwrap();
        assert_eq!(a.payload["text"], "for-a");
        // After consuming "a"'s only message, "a" is empty.
        assert!(mb.recv("a").await.unwrap().is_none());
        // "aa"'s message is still there.
        let aa = mb.recv("aa").await.unwrap().unwrap();
        assert_eq!(aa.payload["text"], "for-aa");
    }

    #[tokio::test]
    async fn redb_backend_preserves_fifo_under_same_ms_burst() {
        // Regression for the M1 finding on PR #298: UUID v7 only encodes
        // 48 bits of ms in its leading bits, so two messages produced in the
        // same millisecond are ordered by random bytes — not send order —
        // unless the redb composite key carries a monotonic tiebreaker.
        //
        // This test sends N messages back-to-back (with no sleep) so they
        // almost certainly share a ms, then asserts `recv` returns them in
        // send order.
        const N: usize = 200;
        let dir = tempfile::tempdir().unwrap();
        let mb = RedbMailbox::open(dir.path()).await.unwrap();

        for i in 0..N {
            mb.send(MailboxMessage::new(
                MailboxMessageType::UserMessage,
                "leader",
                "worker",
                serde_json::json!({ "i": i }),
            ))
            .await
            .unwrap();
        }

        for expected in 0..N {
            let m = mb
                .recv("worker")
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("ran out of messages at index {expected} of {N}"));
            assert_eq!(
                m.payload["i"], expected,
                "FIFO violation at index {expected} — got {}",
                m.payload["i"]
            );
        }
        assert!(mb.recv("worker").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_process_backend_rejects_empty_and_slash_recipients() {
        // L1 regression: both backends must agree on recipient validation.
        let mb = InProcessMailbox::new();
        assert!(mb.send(msg("", "boom")).await.is_err());
        assert!(mb.send(msg("a/b", "boom")).await.is_err());
        assert!(mb.recv("").await.is_err());
        assert!(mb.recv("a/b").await.is_err());
        assert!(mb.list("").await.is_err());
        assert!(mb.list("a/b").await.is_err());
    }
}
