//! Matrix event parser for inbound sync/appservice payloads.
//!
//! Extracts command events and invite events from Matrix JSON payloads,
//! supporting both flat event arrays and `/sync` response structures.
//! Commands are triggered by the `!octos ` prefix or by mentioning the bot.

use serde_json::Value;
use tracing::debug;

/// A parsed Matrix message that should be handled as a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixCommandEvent {
    pub room_id: String,
    pub sender: String,
    pub prompt: String,
    pub event_id: Option<String>,
}

/// Aggregated result of parsing an inbound Matrix payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatrixInboundParseResult {
    pub commands: Vec<MatrixCommandEvent>,
    /// `(room_id, Option<invited_user_id>)` pairs for rooms the bot was invited to.
    pub rooms_to_auto_join: Vec<(String, Option<String>)>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an inbound Matrix payload without knowledge of the bot's own user ID.
pub fn parse_inbound_payload(payload: &Value) -> MatrixInboundParseResult {
    parse_inbound_payload_for_user(payload, None)
}

/// Parse an inbound Matrix payload, optionally filtering by the bot's user ID.
///
/// Collects invite events and message commands from both flat event arrays and
/// the nested `/sync` response structure.
pub fn parse_inbound_payload_for_user(
    payload: &Value,
    self_user_id: Option<&str>,
) -> MatrixInboundParseResult {
    let mut result = MatrixInboundParseResult::default();

    for (event, room_id_hint) in collect_matrix_events(payload) {
        if let Some(invite) = parse_invite_event(event, room_id_hint) {
            debug!(room_id = %invite.0, "parsed invite event");
            result.rooms_to_auto_join.push(invite);
        }

        if let Some(cmd) = parse_message_event_internal(event, room_id_hint, self_user_id, false) {
            debug!(room_id = %cmd.room_id, sender = %cmd.sender, "parsed command event");
            result.commands.push(cmd);
        }
    }

    result
}

/// Parse a single event in appservice mode.
///
/// In appservice mode plain-text messages are accepted even without the
/// `!octos` prefix or a bot mention, as long as the sender is not the bot's
/// own appservice user (prefixed `_octos_`).
pub fn parse_appservice_message_event(
    event: &Value,
    self_user_id: Option<&str>,
) -> Option<MatrixCommandEvent> {
    let sender = event.get("sender")?.as_str()?;

    // Ignore events sent by the appservice ghost users.
    let localpart = matrix_localpart(sender)?;
    if localpart.starts_with("_octos_") {
        return None;
    }

    parse_message_event_internal(event, None, self_user_id, true)
}

/// Parse an invite event, returning `(room_id, Option<invited_user_id>)`.
pub fn parse_invite_event(
    event: &Value,
    room_id_hint: Option<&str>,
) -> Option<(String, Option<String>)> {
    let event_type = event.get("type")?.as_str()?;
    if event_type != "m.room.member" {
        return None;
    }

    let membership = event.get("content")?.get("membership")?.as_str()?;
    if membership != "invite" {
        return None;
    }

    let room_id = event
        .get("room_id")
        .and_then(Value::as_str)
        .or(room_id_hint)?
        .to_string();

    let invited_user = event
        .get("state_key")
        .and_then(Value::as_str)
        .map(String::from);

    Some((room_id, invited_user))
}

// ---------------------------------------------------------------------------
// Internal message parsing
// ---------------------------------------------------------------------------

/// Core message-event parser shared by public entry points.
///
/// When `allow_plain_text` is `true` (appservice mode) the raw body is used as
/// the prompt even when no prefix or mention is found.
fn parse_message_event_internal(
    event: &Value,
    room_id_hint: Option<&str>,
    self_user_id: Option<&str>,
    allow_plain_text: bool,
) -> Option<MatrixCommandEvent> {
    let event_type = event.get("type")?.as_str()?;
    if event_type != "m.room.message" {
        return None;
    }

    let content = event.get("content")?;

    let msgtype = content.get("msgtype")?.as_str()?;
    if msgtype != "m.text" {
        return None;
    }

    let body = content.get("body")?.as_str()?;
    if body.is_empty() {
        return None;
    }

    let prompt = extract_prompt(content, self_user_id).or_else(|| {
        if allow_plain_text {
            Some(body.trim().to_string())
        } else {
            None
        }
    })?;

    if prompt.is_empty() {
        return None;
    }

    let room_id = event
        .get("room_id")
        .and_then(Value::as_str)
        .or(room_id_hint)?
        .to_string();

    let sender = event.get("sender")?.as_str()?.to_string();

    let event_id = event
        .get("event_id")
        .and_then(Value::as_str)
        .map(String::from);

    Some(MatrixCommandEvent {
        room_id,
        sender,
        prompt,
        event_id,
    })
}

// ---------------------------------------------------------------------------
// Prompt extraction
// ---------------------------------------------------------------------------

/// Try to extract a command prompt from the message content.
///
/// Checks the `!octos ` prefix first, then falls back to bot-mention detection.
fn extract_prompt(content: &Value, self_user_id: Option<&str>) -> Option<String> {
    let body = content.get("body")?.as_str()?;

    if let Some(p) = extract_prefixed_prompt(body) {
        return Some(p);
    }

    extract_prompt_from_bot_mention(content, self_user_id)
}

/// Strip the `!octos ` prefix and return the remainder as the prompt.
fn extract_prefixed_prompt(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("!octos ")?;
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    Some(rest.to_string())
}

/// Extract the Matrix localpart from a fully-qualified user ID (`@user:server`).
fn matrix_localpart(user_id: &str) -> Option<&str> {
    let without_sigil = user_id.strip_prefix('@')?;
    Some(without_sigil.split(':').next().unwrap_or(without_sigil))
}

/// Generate mention candidate strings for a Matrix user ID.
///
/// For `@mybot:example.com` this returns `["@mybot:example.com", "mybot"]`.
fn mention_candidates(user_id: &str) -> Vec<String> {
    let mut candidates = vec![user_id.to_string()];
    if let Some(lp) = matrix_localpart(user_id) {
        candidates.push(lp.to_string());
    }
    candidates
}

/// Check whether `text` starts with `candidate` (case-insensitive) and return
/// the remainder after the candidate prefix.
fn split_leading_candidate<'a>(text: &'a str, candidate: &str) -> Option<&'a str> {
    let lower = text.to_lowercase();
    let cand_lower = candidate.to_lowercase();
    if lower.starts_with(&cand_lower) {
        Some(&text[candidate.len()..])
    } else {
        None
    }
}

/// Clean up the text left after stripping a mention/prefix.
///
/// Strips leading punctuation (`:`, `,`, `;`, `>`) and whitespace.
fn cleanup_stripped_prompt(text: &str) -> Option<String> {
    let cleaned = text
        .trim_start_matches(|c: char| ":,;> ".contains(c))
        .trim();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

/// Check whether `m.mentions.user_ids` in the content includes `user_id`.
fn content_mentions_user(content: &Value, user_id: &str) -> bool {
    content
        .get("m.mentions")
        .and_then(|m| m.get("user_ids"))
        .and_then(Value::as_array)
        .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(user_id)))
}

/// Naively strip HTML tags from a string.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut inside_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' if inside_tag => inside_tag = false,
            _ if !inside_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Try to extract a prompt from `formatted_body` by finding and stripping
/// the `<a href="https://matrix.to/...">` mention tag for the bot.
fn extract_prompt_from_formatted_body(formatted_body: &str, user_id: &str) -> Option<String> {
    // Look for a mention anchor: <a href="https://matrix.to/#/@user:server">...</a>
    let mention_href = format!("https://matrix.to/#/{user_id}");
    let lower = formatted_body.to_lowercase();
    let href_lower = mention_href.to_lowercase();

    // Find the <a ...> tag containing the mention href.
    let href_pos = lower.find(&href_lower)?;

    // Walk backwards to find the opening `<a`.
    let before = &lower[..href_pos];
    let tag_start = before.rfind("<a")?;

    // Walk forward from the href to find the closing `</a>`.
    let after_href = &formatted_body[href_pos..];
    let close_tag = after_href.find("</a>")?;
    let end_pos = href_pos + close_tag + "</a>".len();

    // Remove the entire <a ...>...</a> mention from the formatted body.
    let remaining = format!(
        "{}{}",
        &formatted_body[..tag_start],
        &formatted_body[end_pos..]
    );

    let stripped = strip_html_tags(&remaining);
    cleanup_stripped_prompt(&stripped)
}

/// Try to extract a prompt when the bot is mentioned (via body text, formatted
/// body, or `m.mentions`).
fn extract_prompt_from_bot_mention(content: &Value, self_user_id: Option<&str>) -> Option<String> {
    let user_id = self_user_id?;
    let body = content.get("body")?.as_str()?;

    // 1. Check if body starts with one of the mention candidates.
    for candidate in mention_candidates(user_id) {
        if let Some(rest) = split_leading_candidate(body, &candidate) {
            return cleanup_stripped_prompt(rest);
        }
    }

    // 2. Check formatted_body for an <a href> mention tag.
    if let Some(fmt_body) = content.get("formatted_body").and_then(Value::as_str) {
        if let Some(prompt) = extract_prompt_from_formatted_body(fmt_body, user_id) {
            return Some(prompt);
        }
    }

    // 3. Fall back to m.mentions.user_ids — the whole body is the prompt.
    if content_mentions_user(content, user_id) {
        return cleanup_stripped_prompt(body);
    }

    None
}

// ---------------------------------------------------------------------------
// Event collection
// ---------------------------------------------------------------------------

/// Collect all events from a Matrix payload.
///
/// Handles:
/// - A single event object (has a `type` field).
/// - A flat `{ "events": [...] }` array.
/// - A `/sync` response with `rooms.invite` and `rooms.join` sub-structures.
///
/// Returns `(event, Option<room_id_hint>)` pairs.
fn collect_matrix_events(payload: &Value) -> Vec<(&Value, Option<&str>)> {
    let mut events: Vec<(&Value, Option<&str>)> = Vec::new();

    // Single event object.
    if payload.get("type").is_some() {
        events.push((payload, None));
        return events;
    }

    // Flat event array.
    if let Some(arr) = payload.get("events").and_then(Value::as_array) {
        for ev in arr {
            events.push((ev, None));
        }
    }

    // Sync response: rooms.invite.{room_id}.invite_state.events
    if let Some(invite_rooms) = payload
        .get("rooms")
        .and_then(|r| r.get("invite"))
        .and_then(Value::as_object)
    {
        for (room_id, room_data) in invite_rooms {
            if let Some(inv_events) = room_data
                .get("invite_state")
                .and_then(|s| s.get("events"))
                .and_then(Value::as_array)
            {
                for ev in inv_events {
                    events.push((ev, Some(room_id.as_str())));
                }
            }
        }
    }

    // Sync response: rooms.join.{room_id}.timeline.events
    if let Some(join_rooms) = payload
        .get("rooms")
        .and_then(|r| r.get("join"))
        .and_then(Value::as_object)
    {
        for (room_id, room_data) in join_rooms {
            if let Some(tl_events) = room_data
                .get("timeline")
                .and_then(|t| t.get("events"))
                .and_then(Value::as_array)
            {
                for ev in tl_events {
                    events.push((ev, Some(room_id.as_str())));
                }
            }

            // rooms.join.{room_id}.state.events
            if let Some(st_events) = room_data
                .get("state")
                .and_then(|s| s.get("events"))
                .and_then(Value::as_array)
            {
                for ev in st_events {
                    events.push((ev, Some(room_id.as_str())));
                }
            }
        }
    }

    events
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- helpers --

    fn msg_event(room_id: &str, sender: &str, body: &str) -> Value {
        json!({
            "type": "m.room.message",
            "room_id": room_id,
            "sender": sender,
            "event_id": "$evt1",
            "content": {
                "msgtype": "m.text",
                "body": body
            }
        })
    }

    fn invite_event(room_id: &str, invited_user: &str) -> Value {
        json!({
            "type": "m.room.member",
            "room_id": room_id,
            "sender": "@someone:example.com",
            "state_key": invited_user,
            "content": {
                "membership": "invite"
            }
        })
    }

    // -- flat event payload --

    #[test]
    fn should_parse_flat_event_payload_with_invites_and_messages() {
        let payload = json!({
            "events": [
                invite_event("!room1:example.com", "@bot:example.com"),
                msg_event("!room1:example.com", "@alice:example.com", "!octos hello world"),
            ]
        });

        let result = parse_inbound_payload(&payload);

        assert_eq!(result.rooms_to_auto_join.len(), 1);
        assert_eq!(result.rooms_to_auto_join[0].0, "!room1:example.com");
        assert_eq!(
            result.rooms_to_auto_join[0].1.as_deref(),
            Some("@bot:example.com")
        );

        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].room_id, "!room1:example.com");
        assert_eq!(result.commands[0].sender, "@alice:example.com");
        assert_eq!(result.commands[0].prompt, "hello world");
        assert_eq!(result.commands[0].event_id.as_deref(), Some("$evt1"));
    }

    // -- sync payload --

    #[test]
    fn should_parse_sync_payload_with_invites_and_joined_room_messages() {
        let payload = json!({
            "rooms": {
                "invite": {
                    "!invited_room:example.com": {
                        "invite_state": {
                            "events": [
                                {
                                    "type": "m.room.member",
                                    "sender": "@inviter:example.com",
                                    "state_key": "@bot:example.com",
                                    "content": { "membership": "invite" }
                                }
                            ]
                        }
                    }
                },
                "join": {
                    "!joined_room:example.com": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@bob:example.com",
                                    "event_id": "$sync_evt",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "!octos tell me a joke"
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });

        let result = parse_inbound_payload(&payload);

        assert_eq!(result.rooms_to_auto_join.len(), 1);
        assert_eq!(result.rooms_to_auto_join[0].0, "!invited_room:example.com");

        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].room_id, "!joined_room:example.com");
        assert_eq!(result.commands[0].sender, "@bob:example.com");
        assert_eq!(result.commands[0].prompt, "tell me a joke");
    }

    // -- multiple commands from single sync --

    #[test]
    fn should_parse_multiple_commands_from_single_sync_payload() {
        let payload = json!({
            "rooms": {
                "join": {
                    "!room_a:example.com": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@alice:example.com",
                                    "event_id": "$e1",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "!octos first"
                                    }
                                }
                            ]
                        }
                    },
                    "!room_b:example.com": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@bob:example.com",
                                    "event_id": "$e2",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "!octos second"
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });

        let result = parse_inbound_payload(&payload);
        assert_eq!(result.commands.len(), 2);

        let prompts: Vec<&str> = result.commands.iter().map(|c| c.prompt.as_str()).collect();
        assert!(prompts.contains(&"first"));
        assert!(prompts.contains(&"second"));
    }

    // -- leading localpart mention --

    #[test]
    fn should_parse_leading_localpart_mention_for_authenticated_user() {
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@alice:example.com",
            "event_id": "$m1",
            "content": {
                "msgtype": "m.text",
                "body": "mybot: what is 2+2?"
            }
        });

        let payload = json!({ "events": [event] });
        let result = parse_inbound_payload_for_user(&payload, Some("@mybot:example.com"));

        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].prompt, "what is 2+2?");
    }

    // -- formatted_body mention --

    #[test]
    fn should_parse_formatted_body_mention_for_authenticated_user() {
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@alice:example.com",
            "event_id": "$m2",
            "content": {
                "msgtype": "m.text",
                "body": "mybot: explain rust lifetimes",
                "format": "org.matrix.custom.html",
                "formatted_body": "<a href=\"https://matrix.to/#/@mybot:example.com\">mybot</a> explain rust lifetimes"
            }
        });

        let payload = json!({ "events": [event] });
        let result = parse_inbound_payload_for_user(&payload, Some("@mybot:example.com"));

        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].prompt, "explain rust lifetimes");
    }

    // -- mention events without authenticated user still require prefix --

    #[test]
    fn should_require_prefix_when_no_authenticated_user() {
        // Message with a mention but no `!octos` prefix, and no self_user_id.
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@alice:example.com",
            "event_id": "$m3",
            "content": {
                "msgtype": "m.text",
                "body": "mybot: hello there",
                "m.mentions": {
                    "user_ids": ["@mybot:example.com"]
                }
            }
        });

        let payload = json!({ "events": [event] });
        let result = parse_inbound_payload(&payload);

        // Without self_user_id the mention path is not taken, and no prefix
        // is present, so no command should be parsed.
        assert!(result.commands.is_empty());
    }

    // -- appservice: plain text accepted --

    #[test]
    fn should_accept_plain_text_in_appservice_mode() {
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@human:example.com",
            "event_id": "$as1",
            "content": {
                "msgtype": "m.text",
                "body": "just a plain message"
            }
        });

        let cmd = parse_appservice_message_event(&event, None);
        assert!(cmd.is_some());

        let cmd = cmd.unwrap();
        assert_eq!(cmd.prompt, "just a plain message");
        assert_eq!(cmd.room_id, "!room:example.com");
    }

    // -- appservice: strip leading mention --

    #[test]
    fn should_strip_leading_mention_in_appservice_mode() {
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@human:example.com",
            "event_id": "$as2",
            "content": {
                "msgtype": "m.text",
                "body": "mybot: do something cool"
            }
        });

        let cmd = parse_appservice_message_event(&event, Some("@mybot:example.com"));
        assert!(cmd.is_some());

        let cmd = cmd.unwrap();
        assert_eq!(cmd.prompt, "do something cool");
    }

    // -- appservice: ignore bot's own ghost users --

    #[test]
    fn should_ignore_appservice_ghost_users() {
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@_octos_bridge:example.com",
            "event_id": "$as3",
            "content": {
                "msgtype": "m.text",
                "body": "echoed message"
            }
        });

        let cmd = parse_appservice_message_event(&event, None);
        assert!(cmd.is_none());
    }

    // -- empty body ignored --

    #[test]
    fn should_ignore_empty_body() {
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@alice:example.com",
            "event_id": "$empty",
            "content": {
                "msgtype": "m.text",
                "body": ""
            }
        });

        let payload = json!({ "events": [event] });
        let result = parse_inbound_payload(&payload);
        assert!(result.commands.is_empty());
    }

    // -- non-m.text msgtype ignored --

    #[test]
    fn should_ignore_non_text_msgtype() {
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@alice:example.com",
            "event_id": "$img",
            "content": {
                "msgtype": "m.image",
                "body": "photo.png",
                "url": "mxc://example.com/abc"
            }
        });

        let payload = json!({ "events": [event] });
        let result = parse_inbound_payload(&payload);
        assert!(result.commands.is_empty());
    }

    // -- single event object (no wrapping array) --

    #[test]
    fn should_parse_single_event_object() {
        let event = msg_event("!room:example.com", "@alice:example.com", "!octos ping");

        let result = parse_inbound_payload(&event);
        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].prompt, "ping");
    }

    // -- m.mentions.user_ids mention --

    #[test]
    fn should_parse_m_mentions_user_ids() {
        let event = json!({
            "type": "m.room.message",
            "room_id": "!room:example.com",
            "sender": "@alice:example.com",
            "event_id": "$mention",
            "content": {
                "msgtype": "m.text",
                "body": "what is the weather?",
                "m.mentions": {
                    "user_ids": ["@mybot:example.com"]
                }
            }
        });

        let payload = json!({ "events": [event] });
        let result = parse_inbound_payload_for_user(&payload, Some("@mybot:example.com"));

        assert_eq!(result.commands.len(), 1);
        assert_eq!(result.commands[0].prompt, "what is the weather?");
    }

    // -- helper unit tests --

    #[test]
    fn should_extract_matrix_localpart() {
        assert_eq!(matrix_localpart("@alice:example.com"), Some("alice"));
        assert_eq!(matrix_localpart("@bot:matrix.org"), Some("bot"));
        assert_eq!(matrix_localpart("notauser"), None);
    }

    #[test]
    fn should_strip_html_tags() {
        assert_eq!(
            strip_html_tags("<a href=\"url\">text</a> rest"),
            "text rest"
        );
        assert_eq!(strip_html_tags("no tags here"), "no tags here");
        assert_eq!(strip_html_tags("<b>bold</b>"), "bold");
    }

    #[test]
    fn should_extract_prefixed_prompt() {
        assert_eq!(
            extract_prefixed_prompt("!octos do something"),
            Some("do something".to_string())
        );
        assert_eq!(extract_prefixed_prompt("!octos "), None);
        assert_eq!(extract_prefixed_prompt("random text"), None);
        assert_eq!(
            extract_prefixed_prompt("  !octos trimmed  "),
            Some("trimmed".to_string())
        );
    }

    #[test]
    fn should_cleanup_stripped_prompt() {
        assert_eq!(
            cleanup_stripped_prompt(": hello"),
            Some("hello".to_string())
        );
        assert_eq!(
            cleanup_stripped_prompt(",; > text"),
            Some("text".to_string())
        );
        assert_eq!(cleanup_stripped_prompt("  :::  "), None);
    }

    #[test]
    fn should_collect_events_from_sync_state() {
        let payload = json!({
            "rooms": {
                "join": {
                    "!room:example.com": {
                        "state": {
                            "events": [
                                invite_event("!room:example.com", "@bot:example.com")
                            ]
                        }
                    }
                }
            }
        });

        let result = parse_inbound_payload(&payload);
        assert_eq!(result.rooms_to_auto_join.len(), 1);
    }
}
