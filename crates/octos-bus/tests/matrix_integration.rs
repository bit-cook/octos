#![cfg(feature = "matrix-appservice")]

//! Integration tests for the Matrix channel implementations.
//!
//! These tests spin up mock servers and bind network ports, so they are
//! marked `#[ignore]`.  Run them explicitly with:
//!
//! ```sh
//! cargo test -p octos-bus --features matrix-appservice --test matrix_integration -- --ignored
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};

use octos_bus::channel::Channel;
use octos_bus::matrix_appservice::{MatrixAppserviceChannel, MatrixAppserviceConfig};
use octos_bus::matrix_channel::{MatrixChannel, MatrixChannelConfig};
use octos_bus::matrix_client::MatrixClient;

// ---------------------------------------------------------------------------
// Mock Matrix homeserver
// ---------------------------------------------------------------------------

/// A recorded HTTP request: (method, path, body).
type RecordedRequest = (String, String, serde_json::Value);

/// Shared state for the mock homeserver.
#[derive(Clone)]
struct MockState {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    /// Counter tracking the number of `/sync` calls received.
    sync_count: Arc<AtomicUsize>,
}

/// Start a mock Matrix homeserver on a random port and return `(base_url, state)`.
async fn start_mock_homeserver() -> (String, MockState) {
    let state = MockState {
        requests: Arc::new(Mutex::new(Vec::new())),
        sync_count: Arc::new(AtomicUsize::new(0)),
    };

    let app = Router::new()
        .route("/_matrix/client/v3/account/whoami", get(mock_whoami))
        .route("/_matrix/client/v3/sync", get(mock_sync))
        .route("/_matrix/client/v3/join/{room_id}", post(mock_join))
        .route(
            "/_matrix/client/v3/rooms/{room_id}/send/m.room.message/{txn_id}",
            put(mock_send),
        )
        .route("/_matrix/client/v3/joined_rooms", get(mock_joined_rooms))
        .route(
            "/_matrix/client/v3/rooms/{room_id}/typing/{user_id}",
            put(mock_typing),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock homeserver");
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    let base_url = format!("http://127.0.0.1:{port}");
    (base_url, state)
}

// -- mock handlers ----------------------------------------------------------

async fn mock_whoami(State(state): State<MockState>) -> impl IntoResponse {
    state.requests.lock().await.push((
        "GET".into(),
        "/_matrix/client/v3/account/whoami".into(),
        json!({}),
    ));
    axum::Json(json!({ "user_id": "@bot:localhost" }))
}

async fn mock_sync(State(state): State<MockState>) -> impl IntoResponse {
    let call_number = state.sync_count.fetch_add(1, Ordering::SeqCst);
    state
        .requests
        .lock()
        .await
        .push(("GET".into(), "/_matrix/client/v3/sync".into(), json!({})));

    // First call (initial sync, timeout=0): return an empty response with a batch token.
    // Second call: return a sync response containing a message event.
    if call_number == 0 {
        axum::Json(json!({
            "next_batch": "s1_initial",
            "rooms": { "join": {}, "invite": {} }
        }))
    } else {
        axum::Json(json!({
            "next_batch": format!("s{}_after", call_number + 1),
            "rooms": {
                "join": {
                    "!room:localhost": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "room_id": "!room:localhost",
                                    "sender": "@alice:localhost",
                                    "event_id": "$msg_from_sync",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "!octos hello from sync"
                                    }
                                }
                            ]
                        }
                    }
                },
                "invite": {}
            }
        }))
    }
}

async fn mock_join(
    State(state): State<MockState>,
    Path(room_id): Path<String>,
    body: Option<axum::Json<serde_json::Value>>,
) -> impl IntoResponse {
    let body_val = body.map(|b| b.0).unwrap_or(json!({}));
    state.requests.lock().await.push((
        "POST".into(),
        format!("/_matrix/client/v3/join/{room_id}"),
        body_val,
    ));
    axum::Json(json!({ "room_id": "!room:localhost" }))
}

async fn mock_send(
    State(state): State<MockState>,
    Path((room_id, txn_id)): Path<(String, String)>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> impl IntoResponse {
    state.requests.lock().await.push((
        "PUT".into(),
        format!("/_matrix/client/v3/rooms/{room_id}/send/m.room.message/{txn_id}"),
        body,
    ));
    axum::Json(json!({ "event_id": "$evt123" }))
}

async fn mock_joined_rooms(State(state): State<MockState>) -> impl IntoResponse {
    state.requests.lock().await.push((
        "GET".into(),
        "/_matrix/client/v3/joined_rooms".into(),
        json!({}),
    ));
    axum::Json(json!({ "joined_rooms": ["!room:localhost"] }))
}

async fn mock_typing(
    State(state): State<MockState>,
    Path((room_id, user_id)): Path<(String, String)>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> impl IntoResponse {
    state.requests.lock().await.push((
        "PUT".into(),
        format!("/_matrix/client/v3/rooms/{room_id}/typing/{user_id}"),
        body,
    ));
    axum::Json(json!({}))
}

// ---------------------------------------------------------------------------
// Helper: find a random free port
// ---------------------------------------------------------------------------

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind for free port");
    listener.local_addr().unwrap().port()
}

// ---------------------------------------------------------------------------
// Tests — MatrixClient
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn should_authenticate_with_access_token() {
    let (base_url, state) = start_mock_homeserver().await;

    let client = MatrixClient::new(&base_url, "test_token");
    let whoami = client.whoami().await.expect("whoami should succeed");

    assert_eq!(whoami.user_id, "@bot:localhost");

    let requests = state.requests.lock().await;
    assert!(
        requests
            .iter()
            .any(|(m, p, _)| m == "GET" && p.contains("whoami")),
        "mock should have recorded a whoami request",
    );
}

#[tokio::test]
#[ignore]
async fn should_send_text_message_and_return_event_id() {
    let (base_url, state) = start_mock_homeserver().await;

    let client = MatrixClient::new(&base_url, "test_token");
    let event_id = client
        .send_text("!room:localhost", "hello")
        .await
        .expect("send_text should succeed");

    assert_eq!(event_id, "$evt123");

    let requests = state.requests.lock().await;
    let send_req = requests
        .iter()
        .find(|(m, p, _)| m == "PUT" && p.contains("/send/m.room.message/"))
        .expect("mock should have recorded a PUT send request");

    assert_eq!(send_req.2["msgtype"], "m.text");
    assert_eq!(send_req.2["body"], "hello");
}

#[tokio::test]
#[ignore]
async fn should_edit_message_with_replace_relation() {
    let (base_url, state) = start_mock_homeserver().await;

    let client = MatrixClient::new(&base_url, "test_token");
    let new_event_id = client
        .edit_message("!room:localhost", "$orig", "new text")
        .await
        .expect("edit_message should succeed");

    assert_eq!(new_event_id, "$evt123");

    let requests = state.requests.lock().await;
    let edit_req = requests
        .iter()
        .find(|(m, p, _)| m == "PUT" && p.contains("/send/m.room.message/"))
        .expect("mock should have recorded a PUT edit request");

    // The edit body should contain m.new_content and m.relates_to.
    let new_content = &edit_req.2["m.new_content"];
    assert_eq!(new_content["msgtype"], "m.text");
    assert_eq!(new_content["body"], "new text");

    let relates_to = &edit_req.2["m.relates_to"];
    assert_eq!(relates_to["rel_type"], "m.replace");
    assert_eq!(relates_to["event_id"], "$orig");
}

// ---------------------------------------------------------------------------
// Tests — MatrixChannel (user mode)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn should_start_user_mode_channel_and_receive_sync_message() {
    let (base_url, _state) = start_mock_homeserver().await;

    let shutdown = Arc::new(AtomicBool::new(false));
    let channel = MatrixChannel::new(
        MatrixChannelConfig {
            homeserver: base_url,
            access_token: Some("test_token".into()),
            user_id: None,
            password: None,
            device_name: None,
            allowed_senders: vec![],
            allowed_rooms: vec![],
            auto_join: true,
        },
        Arc::clone(&shutdown),
    );

    let (tx, mut rx) = mpsc::channel(16);

    channel
        .start(tx)
        .await
        .expect("channel.start should succeed");

    // Wait for an inbound message from the sync loop (second sync returns a message).
    let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("should receive message within timeout")
        .expect("channel should not be closed");

    assert_eq!(msg.channel, "matrix");
    assert_eq!(msg.sender_id, "@alice:localhost");
    assert_eq!(msg.chat_id, "!room:localhost");
    assert_eq!(msg.content, "hello from sync");
    assert_eq!(msg.message_id.as_deref(), Some("$msg_from_sync"));

    channel.stop().await.expect("stop should succeed");
}

// ---------------------------------------------------------------------------
// Tests — MatrixAppserviceChannel
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn should_start_appservice_and_handle_transaction() {
    let port = free_port().await;

    // The appservice needs a homeserver URL for outbound calls, but for this
    // test we only exercise the inbound HTTP path, so a dummy URL is fine.
    let channel = MatrixAppserviceChannel::new(MatrixAppserviceConfig {
        homeserver: "http://127.0.0.1:1".into(), // not used in this test
        server_name: "localhost".into(),
        appservice_id: "octos-test".into(),
        as_token: "as_secret".into(),
        hs_token: "hs_secret".into(),
        sender_localpart: "_octos_bot".into(),
        user_prefix: "_octos_".into(),
        listen_port: port,
        allowed_senders: vec![],
    });

    let (tx, mut rx) = mpsc::channel(16);
    channel.start(tx).await.expect("start should succeed");

    // Give the appservice HTTP server a moment to start accepting connections.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // POST a transaction with a message event.
    let client = reqwest::Client::new();
    let txn_url =
        format!("http://127.0.0.1:{port}/_matrix/app/v1/transactions/txn1?access_token=hs_secret");
    let resp = client
        .put(&txn_url)
        .json(&json!({
            "events": [
                {
                    "type": "m.room.message",
                    "room_id": "!testroom:localhost",
                    "sender": "@human:localhost",
                    "event_id": "$appservice_evt",
                    "content": {
                        "msgtype": "m.text",
                        "body": "hello from appservice test"
                    }
                }
            ]
        }))
        .send()
        .await
        .expect("PUT transaction should succeed");

    assert_eq!(resp.status(), StatusCode::OK);

    let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("should receive message within timeout")
        .expect("channel should not be closed");

    assert_eq!(msg.channel, "matrix-appservice");
    assert_eq!(msg.sender_id, "@human:localhost");
    assert_eq!(msg.chat_id, "!testroom:localhost");
    assert_eq!(msg.content, "hello from appservice test");
    assert_eq!(msg.message_id.as_deref(), Some("$appservice_evt"));

    channel.stop().await.expect("stop should succeed");
}

#[tokio::test]
#[ignore]
async fn should_reject_appservice_transaction_with_wrong_token() {
    let port = free_port().await;

    let channel = MatrixAppserviceChannel::new(MatrixAppserviceConfig {
        homeserver: "http://127.0.0.1:1".into(),
        server_name: "localhost".into(),
        appservice_id: "octos-test".into(),
        as_token: "as_secret".into(),
        hs_token: "hs_secret".into(),
        sender_localpart: "_octos_bot".into(),
        user_prefix: "_octos_".into(),
        listen_port: port,
        allowed_senders: vec![],
    });

    let (tx, _rx) = mpsc::channel(16);
    channel.start(tx).await.expect("start should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // POST with an incorrect hs_token.
    let client = reqwest::Client::new();
    let txn_url =
        format!("http://127.0.0.1:{port}/_matrix/app/v1/transactions/txn_bad?access_token=WRONG");
    let resp = client
        .put(&txn_url)
        .json(&json!({ "events": [] }))
        .send()
        .await
        .expect("PUT should complete");

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    channel.stop().await.expect("stop should succeed");
}

#[tokio::test]
#[ignore]
async fn should_auto_join_invited_room_via_appservice() {
    // We need a real mock homeserver for the outbound join call.
    let (base_url, mock_state) = start_mock_homeserver().await;
    let port = free_port().await;

    let channel = MatrixAppserviceChannel::new(MatrixAppserviceConfig {
        homeserver: base_url,
        server_name: "localhost".into(),
        appservice_id: "octos-test".into(),
        as_token: "as_secret".into(),
        hs_token: "hs_secret".into(),
        sender_localpart: "_octos_bot".into(),
        user_prefix: "_octos_".into(),
        listen_port: port,
        allowed_senders: vec![],
    });

    let (tx, _rx) = mpsc::channel(16);
    channel.start(tx).await.expect("start should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // POST a transaction containing an invite event.
    let client = reqwest::Client::new();
    let txn_url = format!(
        "http://127.0.0.1:{port}/_matrix/app/v1/transactions/txn_invite?access_token=hs_secret"
    );
    let resp = client
        .put(&txn_url)
        .json(&json!({
            "events": [
                {
                    "type": "m.room.member",
                    "room_id": "!invited_room:localhost",
                    "sender": "@inviter:localhost",
                    "state_key": "@_octos_bot:localhost",
                    "content": {
                        "membership": "invite"
                    }
                }
            ]
        }))
        .send()
        .await
        .expect("PUT transaction should succeed");

    assert_eq!(resp.status(), StatusCode::OK);

    // Give the appservice a moment to issue the outbound join request.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // The mock homeserver should have received a POST /join request.
    let requests = mock_state.requests.lock().await;
    let join_req = requests
        .iter()
        .find(|(m, p, _)| m == "POST" && p.contains("/join/"));
    assert!(
        join_req.is_some(),
        "mock homeserver should have received a join request; recorded: {requests:?}",
    );

    channel.stop().await.expect("stop should succeed");
}
