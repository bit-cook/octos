//! Matrix channel implementing the Channel trait (User mode).
//!
//! Connects to a Matrix homeserver via the Client-Server API, runs a long-poll
//! sync loop for inbound messages, and sends outbound messages to rooms.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use eyre::{Result, WrapErr};
use octos_core::{InboundMessage, OutboundMessage};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::channel::{Channel, ChannelHealth};
use crate::matrix_client::MatrixClient;
use crate::matrix_parse;

/// Configuration for a Matrix channel in User mode.
pub struct MatrixChannelConfig {
    pub homeserver: String,
    pub access_token: Option<String>,
    pub user_id: Option<String>,
    pub password: Option<String>,
    pub device_name: Option<String>,
    pub allowed_senders: Vec<String>,
    pub allowed_rooms: Vec<String>,
    pub auto_join: bool,
}

/// Matrix channel in User mode (single bot account with long-poll sync).
pub struct MatrixChannel {
    config: MatrixChannelConfig,
    allowed_senders: HashSet<String>,
    allowed_rooms: HashSet<String>,
    shutdown: Arc<AtomicBool>,
    resolved_user_id: Mutex<Option<String>>,
    sync_handle: Mutex<Option<JoinHandle<()>>>,
    client: Mutex<Option<Arc<MatrixClient>>>,
}

impl MatrixChannel {
    pub fn new(config: MatrixChannelConfig, shutdown: Arc<AtomicBool>) -> Self {
        let allowed_senders: HashSet<String> = config.allowed_senders.iter().cloned().collect();
        let allowed_rooms: HashSet<String> = config.allowed_rooms.iter().cloned().collect();
        Self {
            config,
            allowed_senders,
            allowed_rooms,
            shutdown,
            resolved_user_id: Mutex::new(None),
            sync_handle: Mutex::new(None),
            client: Mutex::new(None),
        }
    }

    /// Authenticate and return a MatrixClient.
    async fn authenticate(&self) -> Result<(MatrixClient, String)> {
        if let Some(token) = &self.config.access_token {
            if !token.is_empty() {
                let client = MatrixClient::new(&self.config.homeserver, token);
                let whoami = client
                    .whoami()
                    .await
                    .wrap_err("failed to verify access token")?;
                info!(user_id = %whoami.user_id, "Matrix authenticated with access token");
                return Ok((client, whoami.user_id));
            }
        }

        let user_id = self
            .config
            .user_id
            .as_deref()
            .ok_or_else(|| eyre::eyre!("Matrix password login requires user_id"))?;
        let password = self
            .config
            .password
            .as_deref()
            .ok_or_else(|| eyre::eyre!("Matrix requires access_token or password"))?;

        let login = MatrixClient::password_login(
            &self.config.homeserver,
            user_id,
            password,
            self.config.device_name.as_deref(),
        )
        .await
        .wrap_err("Matrix password login failed")?;

        info!(user_id = %login.user_id, "Matrix authenticated with password");
        let client = MatrixClient::new(&self.config.homeserver, &login.access_token);
        Ok((client, login.user_id))
    }

    pub fn is_room_allowed(&self, room_id: &str) -> bool {
        self.allowed_rooms.is_empty() || self.allowed_rooms.contains(room_id)
    }
}

#[async_trait]
impl Channel for MatrixChannel {
    fn name(&self) -> &str {
        "matrix"
    }

    async fn start(&self, inbound_tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let (matrix_client, user_id) = self.authenticate().await?;
        let client = Arc::new(matrix_client);

        *self.resolved_user_id.lock().await = Some(user_id.clone());
        *self.client.lock().await = Some(Arc::clone(&client));

        // Initial sync to get the sync token (don't process old events).
        let initial = client.sync(None, 0).await.wrap_err("initial sync failed")?;
        let mut since = initial
            .get("next_batch")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Process initial invites if auto_join is enabled.
        if self.config.auto_join {
            let parsed = matrix_parse::parse_inbound_payload_for_user(&initial, Some(&user_id));
            for (room_id, _invited_user) in &parsed.rooms_to_auto_join {
                info!(room_id = %room_id, "Auto-joining invited room");
                if let Err(e) = client.join_room(room_id).await {
                    warn!(room_id = %room_id, error = %e, "Failed to auto-join room");
                }
            }
        }

        let rooms = client.get_joined_rooms().await.unwrap_or_default();
        info!(room_count = rooms.len(), "Matrix sync initialized");

        // Spawn the sync loop.
        let shutdown = Arc::clone(&self.shutdown);
        let self_user_id = user_id.clone();
        let auto_join = self.config.auto_join;
        let allowed_senders = self.allowed_senders.clone();
        let allowed_rooms = self.allowed_rooms.clone();
        let sync_client = Arc::clone(&client);

        let handle = tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            let max_backoff = Duration::from_secs(30);

            loop {
                if shutdown.load(Ordering::Acquire) {
                    info!("Matrix sync loop shutting down");
                    break;
                }

                let result = sync_client.sync(since.as_deref(), 30_000).await;

                match result {
                    Ok(response) => {
                        backoff = Duration::from_secs(1);

                        if let Some(token) = response.get("next_batch").and_then(|v| v.as_str()) {
                            since = Some(token.to_string());
                        }

                        let parsed = matrix_parse::parse_inbound_payload_for_user(
                            &response,
                            Some(&self_user_id),
                        );

                        // Auto-join invited rooms.
                        if auto_join {
                            for (room_id, _) in &parsed.rooms_to_auto_join {
                                info!(room_id = %room_id, "Auto-joining invited room");
                                if let Err(e) = sync_client.join_room(room_id).await {
                                    warn!(room_id = %room_id, error = %e, "Failed to auto-join");
                                }
                            }
                        }

                        // Forward commands as inbound messages.
                        for cmd in parsed.commands {
                            if cmd.sender == self_user_id {
                                debug!(event_id = ?cmd.event_id, "Skipping own message");
                                continue;
                            }

                            if !allowed_senders.is_empty() && !allowed_senders.contains(&cmd.sender)
                            {
                                debug!(sender = %cmd.sender, "Sender not in allowed list");
                                continue;
                            }

                            if !allowed_rooms.is_empty() && !allowed_rooms.contains(&cmd.room_id) {
                                debug!(room_id = %cmd.room_id, "Room not in allowed list");
                                continue;
                            }

                            let msg = InboundMessage {
                                channel: "matrix".to_string(),
                                sender_id: cmd.sender,
                                chat_id: cmd.room_id,
                                content: cmd.prompt,
                                timestamp: Utc::now(),
                                media: vec![],
                                metadata: serde_json::json!({}),
                                message_id: cmd.event_id,
                            };

                            if inbound_tx.send(msg).await.is_err() {
                                info!("Inbound channel closed, stopping sync");
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, backoff_secs = backoff.as_secs(), "Matrix sync error");
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(max_backoff);
                    }
                }
            }
        });

        *self.sync_handle.lock().await = Some(handle);
        Ok(())
    }

    async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let client = self.client.lock().await;
        let client = client
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Matrix client not initialized"))?;
        client.send_text(&msg.chat_id, &msg.content).await?;
        Ok(())
    }

    fn is_allowed(&self, sender_id: &str) -> bool {
        self.allowed_senders.is_empty() || self.allowed_senders.contains(sender_id)
    }

    fn max_message_length(&self) -> usize {
        65536
    }

    async fn stop(&self) -> Result<()> {
        if let Some(handle) = self.sync_handle.lock().await.take() {
            handle.abort();
            debug!("Matrix sync task aborted");
        }
        Ok(())
    }

    async fn send_typing(&self, chat_id: &str) -> Result<()> {
        let client = self.client.lock().await;
        let Some(client) = client.as_ref() else {
            return Ok(());
        };
        let user_id = self.resolved_user_id.lock().await;
        let Some(user_id) = user_id.as_ref() else {
            return Ok(());
        };
        let _ = client.send_typing(chat_id, user_id, true, 30_000).await;
        Ok(())
    }

    fn supports_edit(&self) -> bool {
        true
    }

    async fn send_with_id(&self, msg: &OutboundMessage) -> Result<Option<String>> {
        let client = self.client.lock().await;
        let client = client
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Matrix client not initialized"))?;
        let event_id = client.send_text(&msg.chat_id, &msg.content).await?;
        Ok(Some(event_id))
    }

    async fn edit_message(&self, chat_id: &str, message_id: &str, new_content: &str) -> Result<()> {
        let client = self.client.lock().await;
        let client = client
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Matrix client not initialized"))?;
        client
            .edit_message(chat_id, message_id, new_content)
            .await?;
        Ok(())
    }

    fn format_outbound(&self, content: &str) -> String {
        content.to_string()
    }

    async fn health_check(&self) -> Result<ChannelHealth> {
        let client = self.client.lock().await;
        if client.is_none() {
            return Ok(ChannelHealth::Down("not connected".into()));
        }
        Ok(ChannelHealth::Healthy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_channel() -> MatrixChannel {
        MatrixChannel::new(
            MatrixChannelConfig {
                homeserver: "https://matrix.example.com".into(),
                access_token: Some("test_token".into()),
                user_id: None,
                password: None,
                device_name: None,
                allowed_senders: vec![],
                allowed_rooms: vec![],
                auto_join: true,
            },
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[test]
    fn should_return_matrix_channel_name() {
        let ch = test_channel();
        assert_eq!(ch.name(), "matrix");
    }

    #[test]
    fn should_allow_all_senders_when_no_allowlist() {
        let ch = test_channel();
        assert!(ch.is_allowed("anyone"));
    }

    #[test]
    fn should_filter_senders_when_allowlist_set() {
        let ch = MatrixChannel::new(
            MatrixChannelConfig {
                homeserver: "https://matrix.example.com".into(),
                access_token: Some("token".into()),
                user_id: None,
                password: None,
                device_name: None,
                allowed_senders: vec!["@alice:example.com".to_string()],
                allowed_rooms: vec![],
                auto_join: false,
            },
            Arc::new(AtomicBool::new(false)),
        );
        assert!(ch.is_allowed("@alice:example.com"));
        assert!(!ch.is_allowed("@bob:example.com"));
    }

    #[test]
    fn should_allow_all_rooms_when_no_allowlist() {
        let ch = test_channel();
        assert!(ch.is_room_allowed("!any:example.com"));
    }

    #[test]
    fn should_filter_rooms_when_allowlist_set() {
        let ch = MatrixChannel::new(
            MatrixChannelConfig {
                homeserver: "https://matrix.example.com".into(),
                access_token: Some("token".into()),
                user_id: None,
                password: None,
                device_name: None,
                allowed_senders: vec![],
                allowed_rooms: vec!["!allowed:example.com".to_string()],
                auto_join: false,
            },
            Arc::new(AtomicBool::new(false)),
        );
        assert!(ch.is_room_allowed("!allowed:example.com"));
        assert!(!ch.is_room_allowed("!other:example.com"));
    }

    #[test]
    fn should_return_65536_max_message_length() {
        let ch = test_channel();
        assert_eq!(ch.max_message_length(), 65536);
    }

    #[test]
    fn should_support_edit() {
        let ch = test_channel();
        assert!(ch.supports_edit());
    }

    #[tokio::test]
    async fn should_report_down_when_not_connected() {
        let ch = test_channel();
        let health = ch.health_check().await.unwrap();
        assert_eq!(health, ChannelHealth::Down("not connected".into()));
    }

    #[tokio::test]
    async fn should_stop_cleanly_when_no_sync_task() {
        let ch = test_channel();
        assert!(ch.stop().await.is_ok());
    }
}
