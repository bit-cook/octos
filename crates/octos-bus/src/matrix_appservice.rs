//! Matrix Appservice channel implementation.
//!
//! Runs an HTTP server implementing the Matrix Application Service API.
//! The homeserver pushes events to us via `PUT /_matrix/app/v1/transactions/:txn_id`.
//! Outbound messages are sent via the Client-Server API using appservice
//! impersonation (`?user_id=`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use chrono::Utc;
use eyre::Result;
use octos_core::{InboundMessage, OutboundMessage};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::channel::{Channel, ChannelHealth};
use crate::matrix_client::{MatrixClient, percent_encode_path};
use crate::matrix_parse;

/// Configuration for a Matrix Appservice channel.
pub struct MatrixAppserviceConfig {
    /// Homeserver base URL (e.g., `https://matrix.example.com`).
    pub homeserver: String,
    /// The server name portion of Matrix IDs (e.g., `example.com`).
    pub server_name: String,
    /// Appservice registration ID (e.g., `octos-matrix`).
    pub appservice_id: String,
    /// Token the appservice uses to authenticate to the homeserver.
    pub as_token: String,
    /// Token the homeserver uses to authenticate to the appservice.
    pub hs_token: String,
    /// Localpart for the appservice bot user (e.g., `_octos_bot`).
    pub sender_localpart: String,
    /// Prefix for managed user localparts (e.g., `_octos_`).
    pub user_prefix: String,
    /// Port for the appservice HTTP endpoint.
    pub listen_port: u16,
    /// Matrix user IDs allowed to interact with the bot (empty = allow all).
    pub allowed_senders: Vec<String>,
}

/// Matrix Appservice channel.
pub struct MatrixAppserviceChannel {
    inner: Arc<AppserviceInner>,
}

struct AppserviceInner {
    config: MatrixAppserviceConfig,
    client: MatrixClient,
    bot_user_id: String,
    room_users: Mutex<HashMap<String, String>>,
    shutdown: Arc<AtomicBool>,
    server_handle: Mutex<Option<JoinHandle<()>>>,
}

/// Shared state passed to axum handlers.
#[derive(Clone)]
struct AppserviceState {
    inner: Arc<AppserviceInner>,
    inbound_tx: mpsc::Sender<InboundMessage>,
}

/// Query parameters for hs_token validation.
#[derive(Deserialize)]
struct AccessTokenQuery {
    access_token: Option<String>,
}

impl MatrixAppserviceChannel {
    pub fn new(config: MatrixAppserviceConfig) -> Self {
        let bot_user_id = format!("@{}:{}", config.sender_localpart, config.server_name);
        let client = MatrixClient::new(&config.homeserver, &config.as_token);
        Self {
            inner: Arc::new(AppserviceInner {
                config,
                client,
                bot_user_id,
                room_users: Mutex::new(HashMap::new()),
                shutdown: Arc::new(AtomicBool::new(false)),
                server_handle: Mutex::new(None),
            }),
        }
    }

    /// Generate the appservice registration YAML for this configuration.
    pub fn registration_yaml(&self) -> String {
        let c = &self.inner.config;
        format!(
            "id: {id}\n\
             url: http://localhost:{port}\n\
             as_token: {as_token}\n\
             hs_token: {hs_token}\n\
             sender_localpart: {sender}\n\
             rate_limited: false\n\
             namespaces:\n\
             {sp}users:\n\
             {sp}{sp}- exclusive: true\n\
             {sp}{sp}{sp} regex: \"@{prefix}.*:{server}\"\n\
             {sp}aliases:\n\
             {sp}{sp}- exclusive: true\n\
             {sp}{sp}{sp} regex: \"#_{appservice_prefix}.*:{server}\"\n\
             {sp}rooms: []\n",
            id = c.appservice_id,
            port = c.listen_port,
            as_token = c.as_token,
            hs_token = c.hs_token,
            sender = c.sender_localpart,
            prefix = c.user_prefix,
            appservice_prefix = "octos_",
            server = c.server_name,
            sp = "  ",
        )
    }

    /// Bind a room to a specific user for impersonation.
    pub async fn bind_room_user(&self, room_id: &str, user_id: &str) {
        self.inner
            .room_users
            .lock()
            .await
            .insert(room_id.to_string(), user_id.to_string());
    }

    /// Get the effective user ID for a room (bound user or bot).
    pub async fn effective_user_id(&self, room_id: &str) -> String {
        self.inner
            .room_users
            .lock()
            .await
            .get(room_id)
            .cloned()
            .unwrap_or_else(|| self.inner.bot_user_id.clone())
    }

    /// Extract the agent slug from a fully-qualified user ID.
    ///
    /// Given `@_octos_default:example.com` with prefix `_octos_` and server
    /// `example.com`, returns `Some("default")`.
    pub fn extract_agent_slug(&self, user_id: &str) -> Option<String> {
        let suffix = format!(":{}", self.inner.config.server_name);

        // Must start with `@{user_prefix}` and end with `:{server_name}`
        let without_sigil = user_id.strip_prefix('@')?;
        let without_suffix = without_sigil.strip_suffix(suffix.as_str())?;
        let slug = without_suffix.strip_prefix(self.inner.config.user_prefix.as_str())?;
        if slug.is_empty() {
            return None;
        }
        Some(slug.to_string())
    }

    /// Check whether a user ID belongs to the appservice namespace.
    pub fn matches_user_namespace(&self, user_id: &str) -> bool {
        let prefix = format!("@{}", self.inner.config.user_prefix);
        let suffix = format!(":{}", self.inner.config.server_name);
        user_id.starts_with(&prefix) && user_id.ends_with(&suffix)
    }
}

// ---------------------------------------------------------------------------
// Appservice impersonation helpers (on AppserviceInner)
// ---------------------------------------------------------------------------

impl AppserviceInner {
    /// Send a text message as a specific user via appservice impersonation.
    async fn send_as_user(&self, room_id: &str, user_id: &str, text: &str) -> Result<String> {
        let txn_id = uuid::Uuid::now_v7().to_string();
        let path = format!(
            "/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            percent_encode_path(room_id),
            txn_id,
        );
        let body = json!({
            "msgtype": "m.text",
            "body": text,
        });
        let resp = self
            .client
            .request_as_user(Method::PUT, &path, user_id, Some(&body))
            .await?;
        let event_id = resp
            .get("event_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(event_id)
    }

    /// Edit a message as a specific user via appservice impersonation.
    async fn edit_as_user(
        &self,
        room_id: &str,
        user_id: &str,
        event_id: &str,
        new_text: &str,
    ) -> Result<String> {
        let txn_id = uuid::Uuid::now_v7().to_string();
        let path = format!(
            "/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            percent_encode_path(room_id),
            txn_id,
        );
        let body = json!({
            "msgtype": "m.text",
            "body": format!("* {new_text}"),
            "m.new_content": {
                "msgtype": "m.text",
                "body": new_text,
            },
            "m.relates_to": {
                "rel_type": "m.replace",
                "event_id": event_id,
            },
        });
        let resp = self
            .client
            .request_as_user(Method::PUT, &path, user_id, Some(&body))
            .await?;
        let event_id = resp
            .get("event_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(event_id)
    }

    /// Send typing indicator as a specific user via appservice impersonation.
    async fn send_typing_as_user(&self, room_id: &str, user_id: &str) -> Result<()> {
        let path = format!(
            "/_matrix/client/v3/rooms/{}/typing/{}",
            percent_encode_path(room_id),
            percent_encode_path(user_id),
        );
        let body = json!({
            "typing": true,
            "timeout": 30000,
        });
        self.client
            .request_as_user(Method::PUT, &path, user_id, Some(&body))
            .await?;
        Ok(())
    }

    /// Auto-join a room as a specific user via appservice impersonation.
    async fn auto_join_as_user(&self, room_id: &str, user_id: &str) -> Result<()> {
        let path = format!("/_matrix/client/v3/join/{}", percent_encode_path(room_id),);
        self.client
            .request_as_user(Method::POST, &path, user_id, Some(&json!({})))
            .await?;
        Ok(())
    }

    /// Get the effective user ID for a room (bound user or bot).
    async fn effective_user_id(&self, room_id: &str) -> String {
        self.room_users
            .lock()
            .await
            .get(room_id)
            .cloned()
            .unwrap_or_else(|| self.bot_user_id.clone())
    }
}

// ---------------------------------------------------------------------------
// Channel trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Channel for MatrixAppserviceChannel {
    fn name(&self) -> &str {
        "matrix-appservice"
    }

    async fn start(&self, inbound_tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let state = AppserviceState {
            inner: Arc::clone(&self.inner),
            inbound_tx,
        };

        let app = Router::new()
            .route(
                "/_matrix/app/v1/transactions/{txn_id}",
                put(handle_transactions),
            )
            .route("/_matrix/app/v1/users/{user_id}", get(handle_user_query))
            .route("/_matrix/app/v1/rooms/{room_alias}", get(handle_room_query))
            .route("/_matrix/app/v1/ping", post(handle_ping))
            .with_state(state);

        let addr = format!("0.0.0.0:{}", self.inner.config.listen_port);
        info!(
            port = self.inner.config.listen_port,
            "Matrix appservice listening on {addr}"
        );
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        let shutdown = Arc::clone(&self.inner.shutdown);
        let handle = tokio::spawn(async move {
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    while !shutdown.load(Ordering::Relaxed) {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                })
                .await;

            if let Err(e) = result {
                warn!(error = %e, "Matrix appservice server error");
            }
            info!("Matrix appservice server stopped");
        });

        *self.inner.server_handle.lock().await = Some(handle);
        Ok(())
    }

    async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let user_id = self.inner.effective_user_id(&msg.chat_id).await;
        self.inner
            .send_as_user(&msg.chat_id, &user_id, &msg.content)
            .await?;
        Ok(())
    }

    fn is_allowed(&self, sender_id: &str) -> bool {
        self.inner.config.allowed_senders.is_empty()
            || self
                .inner
                .config
                .allowed_senders
                .contains(&sender_id.to_string())
    }

    fn max_message_length(&self) -> usize {
        65536
    }

    async fn stop(&self) -> Result<()> {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.inner.server_handle.lock().await.take() {
            handle.abort();
            debug!("Matrix appservice server task aborted");
        }
        Ok(())
    }

    async fn send_typing(&self, chat_id: &str) -> Result<()> {
        let user_id = self.inner.effective_user_id(chat_id).await;
        let _ = self.inner.send_typing_as_user(chat_id, &user_id).await;
        Ok(())
    }

    fn supports_edit(&self) -> bool {
        true
    }

    async fn send_with_id(&self, msg: &OutboundMessage) -> Result<Option<String>> {
        let user_id = self.inner.effective_user_id(&msg.chat_id).await;
        let event_id = self
            .inner
            .send_as_user(&msg.chat_id, &user_id, &msg.content)
            .await?;
        Ok(Some(event_id))
    }

    async fn edit_message(&self, chat_id: &str, message_id: &str, new_content: &str) -> Result<()> {
        let user_id = self.inner.effective_user_id(chat_id).await;
        self.inner
            .edit_as_user(chat_id, &user_id, message_id, new_content)
            .await?;
        Ok(())
    }

    async fn health_check(&self) -> Result<ChannelHealth> {
        let handle = self.inner.server_handle.lock().await;
        if handle.is_some() {
            Ok(ChannelHealth::Healthy)
        } else {
            Ok(ChannelHealth::Down("server not running".into()))
        }
    }
}

// ---------------------------------------------------------------------------
// Axum handlers
// ---------------------------------------------------------------------------

/// Validate the `access_token` query parameter against the expected `hs_token`.
fn validate_hs_token(query: &AccessTokenQuery, expected: &str) -> Result<(), StatusCode> {
    match &query.access_token {
        Some(token) if token == expected => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// `PUT /_matrix/app/v1/transactions/:txn_id`
///
/// Main event handler. The homeserver pushes events here.
async fn handle_transactions(
    State(state): State<AppserviceState>,
    Path(_txn_id): Path<String>,
    query: Query<AccessTokenQuery>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(status) = validate_hs_token(&query, &state.inner.config.hs_token) {
        return (status, axum::Json(json!({}))).into_response();
    }

    let events = body
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for event in &events {
        // Handle invite events — auto-join as the invited user.
        if let Some((room_id, invited_user)) = matrix_parse::parse_invite_event(event, None) {
            let user_to_join = invited_user.as_deref().unwrap_or(&state.inner.bot_user_id);

            info!(
                room_id = %room_id,
                user_id = %user_to_join,
                "Auto-joining invited room",
            );

            if let Err(e) = state.inner.auto_join_as_user(&room_id, user_to_join).await {
                warn!(
                    room_id = %room_id,
                    error = %e,
                    "Failed to auto-join room",
                );
            }

            // If the invited user is a managed appservice user, bind the room.
            if let Some(ref invited) = invited_user {
                let prefix = format!("@{}", state.inner.config.user_prefix);
                let suffix = format!(":{}", state.inner.config.server_name);
                if invited.starts_with(&prefix) && invited.ends_with(&suffix) {
                    state
                        .inner
                        .room_users
                        .lock()
                        .await
                        .insert(room_id.clone(), invited.clone());
                    debug!(
                        room_id = %room_id,
                        user_id = %invited,
                        "Bound room to appservice user",
                    );
                }
            }
        }

        // Handle membership events for room-user binding tracking.
        if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
            if event_type == "m.room.member" {
                if let (Some(room_id), Some(state_key), Some(membership)) = (
                    event.get("room_id").and_then(|v| v.as_str()),
                    event.get("state_key").and_then(|v| v.as_str()),
                    event
                        .get("content")
                        .and_then(|c| c.get("membership"))
                        .and_then(|v| v.as_str()),
                ) {
                    let prefix = format!("@{}", state.inner.config.user_prefix);
                    let suffix = format!(":{}", state.inner.config.server_name);
                    if state_key.starts_with(&prefix) && state_key.ends_with(&suffix) {
                        match membership {
                            "join" => {
                                state
                                    .inner
                                    .room_users
                                    .lock()
                                    .await
                                    .insert(room_id.to_string(), state_key.to_string());
                            }
                            "leave" | "ban" => {
                                state.inner.room_users.lock().await.remove(room_id);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Handle message events — parse and forward as InboundMessage.
        if let Some(cmd) =
            matrix_parse::parse_appservice_message_event(event, Some(&state.inner.bot_user_id))
        {
            // Build metadata, potentially including forced_agent_id.
            let mut metadata = json!({});

            // Check if the room is bound to a managed user, and extract agent slug.
            let room_users = state.inner.room_users.lock().await;
            if let Some(bound_user) = room_users.get(&cmd.room_id) {
                let prefix = format!("@{}", state.inner.config.user_prefix);
                let suffix = format!(":{}", state.inner.config.server_name);
                if bound_user.starts_with(&prefix) && bound_user.ends_with(&suffix) {
                    // Extract the slug between prefix and suffix.
                    let without_sigil = bound_user.trim_start_matches('@');
                    let without_prefix =
                        without_sigil.strip_prefix(&state.inner.config.user_prefix);
                    let suffix_no_colon = suffix.trim_start_matches(':');
                    if let Some(rest) = without_prefix {
                        if let Some(slug) = rest.strip_suffix(suffix_no_colon) {
                            let slug = slug.trim_end_matches(':');
                            if !slug.is_empty() {
                                metadata["forced_agent_id"] = json!(slug);
                            }
                        }
                    }
                }
            }
            drop(room_users);

            let msg = InboundMessage {
                channel: "matrix-appservice".to_string(),
                sender_id: cmd.sender,
                chat_id: cmd.room_id,
                content: cmd.prompt,
                timestamp: Utc::now(),
                media: vec![],
                metadata,
                message_id: cmd.event_id,
            };

            if state.inbound_tx.send(msg).await.is_err() {
                warn!("Inbound channel closed, cannot forward message");
            }
        }
    }

    (StatusCode::OK, axum::Json(json!({}))).into_response()
}

/// `GET /_matrix/app/v1/users/:user_id`
///
/// Homeserver asks whether we claim this user ID.
async fn handle_user_query(
    State(state): State<AppserviceState>,
    Path(user_id): Path<String>,
    query: Query<AccessTokenQuery>,
) -> impl IntoResponse {
    if let Err(status) = validate_hs_token(&query, &state.inner.config.hs_token) {
        return (status, axum::Json(json!({}))).into_response();
    }

    let prefix = format!("@{}", state.inner.config.user_prefix);
    let suffix = format!(":{}", state.inner.config.server_name);
    if user_id.starts_with(&prefix) && user_id.ends_with(&suffix) {
        debug!(user_id = %user_id, "User query matched namespace");
        (StatusCode::OK, axum::Json(json!({}))).into_response()
    } else {
        (StatusCode::NOT_FOUND, axum::Json(json!({}))).into_response()
    }
}

/// `GET /_matrix/app/v1/rooms/:room_alias`
///
/// Homeserver asks whether we claim this room alias. We don't claim any.
async fn handle_room_query(
    State(state): State<AppserviceState>,
    Path(_room_alias): Path<String>,
    query: Query<AccessTokenQuery>,
) -> impl IntoResponse {
    if let Err(status) = validate_hs_token(&query, &state.inner.config.hs_token) {
        return (status, axum::Json(json!({}))).into_response();
    }

    (StatusCode::NOT_FOUND, axum::Json(json!({}))).into_response()
}

/// `POST /_matrix/app/v1/ping`
///
/// Health check endpoint from the homeserver.
async fn handle_ping(
    State(state): State<AppserviceState>,
    query: Query<AccessTokenQuery>,
) -> impl IntoResponse {
    if let Err(status) = validate_hs_token(&query, &state.inner.config.hs_token) {
        return (status, axum::Json(json!({}))).into_response();
    }

    (StatusCode::OK, axum::Json(json!({}))).into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> MatrixAppserviceConfig {
        MatrixAppserviceConfig {
            homeserver: "https://matrix.example.com".into(),
            server_name: "example.com".into(),
            appservice_id: "octos-matrix".into(),
            as_token: "as_token_secret".into(),
            hs_token: "hs_token_secret".into(),
            sender_localpart: "_octos_bot".into(),
            user_prefix: "_octos_".into(),
            listen_port: 8009,
            allowed_senders: vec![],
        }
    }

    fn test_channel() -> MatrixAppserviceChannel {
        MatrixAppserviceChannel::new(test_config())
    }

    #[test]
    fn test_name_returns_matrix_appservice() {
        let ch = test_channel();
        assert_eq!(ch.name(), "matrix-appservice");
    }

    #[test]
    fn test_supports_edit() {
        let ch = test_channel();
        assert!(ch.supports_edit());
    }

    #[tokio::test]
    async fn test_effective_user_id_without_binding() {
        let ch = test_channel();
        let user = ch.effective_user_id("!room:example.com").await;
        assert_eq!(user, "@_octos_bot:example.com");
    }

    #[tokio::test]
    async fn test_effective_user_id_with_binding() {
        let ch = test_channel();
        ch.bind_room_user("!room:example.com", "@_octos_agent1:example.com")
            .await;
        let user = ch.effective_user_id("!room:example.com").await;
        assert_eq!(user, "@_octos_agent1:example.com");
    }

    #[test]
    fn test_registration_yaml_format() {
        let ch = test_channel();
        let yaml = ch.registration_yaml();
        assert!(yaml.contains("id: octos-matrix"));
        assert!(yaml.contains("url: http://localhost:8009"));
        assert!(yaml.contains("as_token: as_token_secret"));
        assert!(yaml.contains("hs_token: hs_token_secret"));
        assert!(yaml.contains("sender_localpart: _octos_bot"));
        assert!(yaml.contains("rate_limited: false"));
        assert!(yaml.contains("@_octos_.*:example.com"));
        assert!(yaml.contains("rooms: []"));
    }

    #[test]
    fn test_extract_agent_slug() {
        let ch = test_channel();
        assert_eq!(
            ch.extract_agent_slug("@_octos_default:example.com"),
            Some("default".to_string()),
        );
        assert_eq!(
            ch.extract_agent_slug("@_octos_my_agent:example.com"),
            Some("my_agent".to_string()),
        );
        // Not matching prefix
        assert_eq!(ch.extract_agent_slug("@other:example.com"), None);
        // Wrong server
        assert_eq!(ch.extract_agent_slug("@_octos_default:other.com"), None);
        // Empty slug (just prefix + server)
        assert_eq!(ch.extract_agent_slug("@_octos_:example.com"), None);
    }

    #[test]
    fn test_user_query_matching() {
        let ch = test_channel();
        assert!(ch.matches_user_namespace("@_octos_foo:example.com"));
        assert!(ch.matches_user_namespace("@_octos_bot:example.com"));
        assert!(!ch.matches_user_namespace("@other:example.com"));
        assert!(!ch.matches_user_namespace("@_octos_foo:other.com"));
    }

    #[test]
    fn test_max_message_length() {
        let ch = test_channel();
        assert_eq!(ch.max_message_length(), 65536);
    }

    #[test]
    fn test_is_allowed_all_when_empty() {
        let ch = test_channel();
        assert!(ch.is_allowed("@anyone:example.com"));
    }

    #[test]
    fn test_is_allowed_filters_when_set() {
        let ch = MatrixAppserviceChannel::new(MatrixAppserviceConfig {
            allowed_senders: vec!["@alice:example.com".to_string()],
            ..test_config()
        });
        assert!(ch.is_allowed("@alice:example.com"));
        assert!(!ch.is_allowed("@bob:example.com"));
    }

    #[tokio::test]
    async fn test_health_check_down_when_no_server() {
        let ch = test_channel();
        let health = ch.health_check().await.unwrap();
        assert_eq!(health, ChannelHealth::Down("server not running".into()));
    }

    #[tokio::test]
    async fn test_stop_when_no_server() {
        let ch = test_channel();
        assert!(ch.stop().await.is_ok());
    }

    #[test]
    fn test_bot_user_id_format() {
        let ch = test_channel();
        assert_eq!(ch.inner.bot_user_id, "@_octos_bot:example.com");
    }

    #[test]
    fn test_validate_hs_token_accepts_correct() {
        let query = AccessTokenQuery {
            access_token: Some("secret".into()),
        };
        assert!(validate_hs_token(&query, "secret").is_ok());
    }

    #[test]
    fn test_validate_hs_token_rejects_wrong() {
        let query = AccessTokenQuery {
            access_token: Some("wrong".into()),
        };
        assert_eq!(
            validate_hs_token(&query, "secret").unwrap_err(),
            StatusCode::UNAUTHORIZED,
        );
    }

    #[test]
    fn test_validate_hs_token_rejects_missing() {
        let query = AccessTokenQuery { access_token: None };
        assert_eq!(
            validate_hs_token(&query, "secret").unwrap_err(),
            StatusCode::UNAUTHORIZED,
        );
    }
}
