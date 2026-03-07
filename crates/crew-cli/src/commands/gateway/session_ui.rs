//! Session UI helpers: inline keyboards, session list text.

/// Escape HTML special characters for Telegram's HTML parse mode.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Truncate a string to `max` chars, appending "…" if truncated.
pub fn truncate_button_text(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

/// Build an inline keyboard JSON value for session selection.
/// 2 buttons per row, active session marked with `>> name <<`.
/// Caps at 50 sessions to stay within Telegram limits.
pub fn build_session_keyboard(
    entries: &[crew_bus::SessionListEntry],
    active_topic: &str,
) -> serde_json::Value {
    let cap = entries.len().min(50);
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut row: Vec<serde_json::Value> = Vec::new();

    for entry in entries.iter().take(cap) {
        let topic = entry.topic.as_deref().unwrap_or("");
        let display_name = if topic.is_empty() { "default" } else { topic };
        let label = if topic == active_topic {
            format!(">> {} <<", truncate_button_text(display_name, 14))
        } else {
            truncate_button_text(display_name, 18)
        };
        let callback_data = format!("s:{topic}");

        row.push(serde_json::json!({
            "text": label,
            "callback_data": callback_data,
        }));

        if row.len() == 2 {
            rows.push(serde_json::Value::Array(row));
            row = Vec::new();
        }
    }
    // Push remaining button if odd count
    if !row.is_empty() {
        rows.push(serde_json::Value::Array(row));
    }

    serde_json::json!({ "inline_keyboard": rows })
}

/// Build an HTML-formatted text listing sessions.
pub fn build_session_text(entries: &[crew_bus::SessionListEntry], active_topic: &str) -> String {
    if entries.is_empty() {
        return "No sessions yet. Send a message to start one.".to_string();
    }

    let mut lines = Vec::new();
    lines.push("<b>Sessions</b>".to_string());
    lines.push(String::new());

    for entry in entries {
        let topic = entry.topic.as_deref().unwrap_or("");
        let display_name = if topic.is_empty() { "default" } else { topic };
        let marker = if topic == active_topic { " ✦" } else { "" };
        let summary = entry.summary.as_deref().unwrap_or("(no summary)");
        let count = entry.message_count;
        // HTML-escape user content to prevent parse errors
        let safe_name = html_escape(display_name);
        let safe_summary = html_escape(summary);
        lines.push(format!(
            "• <b>{safe_name}</b>{marker} — {count} msgs\n  <i>{safe_summary}</i>",
        ));
    }

    lines.join("\n")
}
