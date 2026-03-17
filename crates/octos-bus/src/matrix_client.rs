//! Thin async Matrix client over reqwest.

use std::time::Duration;

use eyre::{Result, bail};
use reqwest::Method;
use serde::Deserialize;
use tracing::warn;

pub(crate) fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhoamiResponse {
    pub user_id: String,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub user_id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SendResponse {
    event_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JoinedRoomsResponse {
    joined_rooms: Vec<String>,
}

pub struct MatrixClient {
    http: reqwest::Client,
    homeserver: String,
    access_token: String,
}

impl MatrixClient {
    pub fn new(homeserver: &str, access_token: &str) -> Self {
        let homeserver = homeserver.trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self {
            http,
            homeserver,
            access_token: access_token.to_string(),
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.homeserver, path)
    }

    fn authed_request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, self.api_url(path))
            .bearer_auth(&self.access_token)
    }

    pub async fn whoami(&self) -> Result<WhoamiResponse> {
        let resp = self
            .authed_request(Method::GET, "/_matrix/client/v3/account/whoami")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "whoami failed");
            bail!("whoami failed ({}): {}", status, body);
        }

        Ok(resp.json().await?)
    }

    pub async fn password_login(
        homeserver: &str,
        user_id: &str,
        password: &str,
        device_name: Option<&str>,
    ) -> Result<LoginResponse> {
        let homeserver = homeserver.trim_end_matches('/');
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        let url = format!("{}/_matrix/client/v3/login", homeserver);

        let mut body = serde_json::json!({
            "type": "m.login.password",
            "identifier": {
                "type": "m.id.user",
                "user": user_id,
            },
            "password": password,
        });

        if let Some(name) = device_name {
            body["initial_device_display_name"] = serde_json::json!(name);
        }

        let resp = http.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "password login failed");
            bail!("password login failed ({}): {}", status, body);
        }

        Ok(resp.json().await?)
    }

    pub async fn sync(&self, since: Option<&str>, timeout_ms: u32) -> Result<serde_json::Value> {
        let mut query = vec![("timeout", timeout_ms.to_string())];
        if let Some(since) = since {
            query.push(("since", since.to_string()));
        }

        let resp = self
            .authed_request(Method::GET, "/_matrix/client/v3/sync")
            .query(&query)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "sync failed");
            bail!("sync failed ({}): {}", status, body);
        }

        Ok(resp.json().await?)
    }

    pub async fn join_room(&self, room_id: &str) -> Result<()> {
        let encoded = percent_encode_path(room_id);
        let path = format!("/_matrix/client/v3/join/{encoded}");

        let resp = self
            .authed_request(Method::POST, &path)
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, room_id = %room_id, "join_room failed");
            bail!("join_room failed ({}): {}", status, body);
        }

        Ok(())
    }

    pub async fn send_text(&self, room_id: &str, text: &str) -> Result<String> {
        let txn_id = uuid::Uuid::now_v7().to_string();
        let encoded_room = percent_encode_path(room_id);
        let path = format!("/_matrix/client/v3/rooms/{encoded_room}/send/m.room.message/{txn_id}");

        let body = serde_json::json!({
            "msgtype": "m.text",
            "body": text,
        });

        let resp = self
            .authed_request(Method::PUT, &path)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "send_text failed");
            bail!("send_text failed ({}): {}", status, body);
        }

        let send: SendResponse = resp.json().await?;
        Ok(send.event_id)
    }

    pub async fn send_html(&self, room_id: &str, text: &str, html: &str) -> Result<String> {
        let txn_id = uuid::Uuid::now_v7().to_string();
        let encoded_room = percent_encode_path(room_id);
        let path = format!("/_matrix/client/v3/rooms/{encoded_room}/send/m.room.message/{txn_id}");

        let body = serde_json::json!({
            "msgtype": "m.text",
            "body": text,
            "format": "org.matrix.custom.html",
            "formatted_body": html,
        });

        let resp = self
            .authed_request(Method::PUT, &path)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "send_html failed");
            bail!("send_html failed ({}): {}", status, body);
        }

        let send: SendResponse = resp.json().await?;
        Ok(send.event_id)
    }

    pub async fn send_typing(
        &self,
        room_id: &str,
        user_id: &str,
        typing: bool,
        timeout_ms: u32,
    ) -> Result<()> {
        let encoded_room = percent_encode_path(room_id);
        let encoded_user = percent_encode_path(user_id);
        let path = format!("/_matrix/client/v3/rooms/{encoded_room}/typing/{encoded_user}");

        let body = serde_json::json!({
            "typing": typing,
            "timeout": timeout_ms,
        });

        let resp = self
            .authed_request(Method::PUT, &path)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "send_typing failed");
            bail!("send_typing failed ({}): {}", status, body);
        }

        Ok(())
    }

    pub async fn edit_message(
        &self,
        room_id: &str,
        event_id: &str,
        new_text: &str,
    ) -> Result<String> {
        let txn_id = uuid::Uuid::now_v7().to_string();
        let encoded_room = percent_encode_path(room_id);
        let path = format!("/_matrix/client/v3/rooms/{encoded_room}/send/m.room.message/{txn_id}");

        let body = serde_json::json!({
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
            .authed_request(Method::PUT, &path)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "edit_message failed");
            bail!("edit_message failed ({}): {}", status, body);
        }

        let send: SendResponse = resp.json().await?;
        Ok(send.event_id)
    }

    pub async fn get_joined_rooms(&self) -> Result<Vec<String>> {
        let resp = self
            .authed_request(Method::GET, "/_matrix/client/v3/joined_rooms")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "get_joined_rooms failed");
            bail!("get_joined_rooms failed ({}): {}", status, body);
        }

        let parsed: JoinedRoomsResponse = resp.json().await?;
        Ok(parsed.joined_rooms)
    }

    pub async fn request_as_user(
        &self,
        method: Method,
        path: &str,
        user_id: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(path);
        let mut req = self
            .http
            .request(method, url)
            .bearer_auth(&self.access_token)
            .query(&[("user_id", user_id)]);

        if let Some(body) = body {
            req = req.json(body);
        }

        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "request_as_user failed");
            bail!("request_as_user failed ({}): {}", status, body);
        }

        Ok(resp.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_strips_trailing_slash() {
        let client = MatrixClient::new("https://matrix.org/", "token");
        assert_eq!(client.homeserver, "https://matrix.org");

        let client = MatrixClient::new("https://matrix.org", "token");
        assert_eq!(client.homeserver, "https://matrix.org");

        let client = MatrixClient::new("https://matrix.org///", "token");
        assert_eq!(client.homeserver, "https://matrix.org");
    }

    #[test]
    fn test_api_url_construction() {
        let client = MatrixClient::new("https://matrix.org", "token");
        assert_eq!(
            client.api_url("/_matrix/client/v3/sync"),
            "https://matrix.org/_matrix/client/v3/sync"
        );
    }
}
