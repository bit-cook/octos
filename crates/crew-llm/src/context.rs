//! Context window limits and token estimation.

use crew_core::Message;

/// Known context window sizes (in tokens) for common models.
///
/// These are best-effort defaults that may become stale as providers update
/// their models. Providers can override `LlmProvider::context_window()` to
/// return accurate values from API metadata or configuration.
pub fn context_window_tokens(model_id: &str) -> u32 {
    let m = model_id.to_lowercase();
    match () {
        // Anthropic Claude
        _ if m.contains("claude-opus-4") || m.contains("claude-sonnet-4") => 200_000,
        _ if m.contains("claude-3") => 200_000,
        // OpenAI
        _ if m.contains("gpt-4o") || m.contains("gpt-4-turbo") => 128_000,
        _ if m.contains("o1") || m.contains("o3") || m.contains("o4") => 200_000,
        _ if m.contains("gpt-4") => 128_000,
        _ if m.contains("gpt-3.5") => 16_385,
        // Google Gemini
        _ if m.contains("gemini-2") || m.contains("gemini-1.5") => 1_000_000,
        _ if m.contains("gemini") => 128_000,
        // DeepSeek
        _ if m.contains("deepseek") => 128_000,
        // Moonshot / Kimi
        _ if m.contains("kimi") || m.contains("moonshot") => 128_000,
        // Qwen / DashScope
        _ if m.contains("qwen") => 128_000,
        // Zhipu / GLM
        _ if m.contains("glm") || m.contains("zhipu") => 128_000,
        // MiniMax
        _ if m.contains("minimax") => 128_000,
        // Local (Llama, etc.)
        _ if m.contains("llama") => 128_000,
        // Conservative default for unknown models
        _ => 128_000,
    }
}

/// Estimate token count from text using character heuristic.
///
/// Uses ~4 chars/token for ASCII (English/code) and ~1.5 chars/token for
/// non-ASCII (CJK, emoji, etc.). This is a rough guard — not a precise
/// tokenizer — so it intentionally overestimates slightly to be safe.
pub fn estimate_tokens(text: &str) -> u32 {
    let ascii_chars = text.bytes().filter(|b| b.is_ascii()).count() as u32;
    let non_ascii_chars = text.chars().count() as u32 - ascii_chars;
    let tokens = ascii_chars / 4 + (non_ascii_chars as f32 / 1.5) as u32;
    tokens.max(1)
}

/// Estimate tokens for a message (content + serialized tool calls + overhead).
pub fn estimate_message_tokens(msg: &Message) -> u32 {
    let mut tokens = estimate_tokens(&msg.content);
    if let Some(ref calls) = msg.tool_calls {
        for call in calls {
            tokens += estimate_tokens(&call.name);
            tokens += estimate_tokens(&call.arguments.to_string());
        }
    }
    // Role/structural overhead (~4 tokens)
    tokens + 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_window_claude() {
        assert_eq!(context_window_tokens("claude-sonnet-4-20250514"), 200_000);
        assert_eq!(context_window_tokens("claude-opus-4-20250514"), 200_000);
    }

    #[test]
    fn test_context_window_openai() {
        assert_eq!(context_window_tokens("gpt-4o"), 128_000);
        assert_eq!(context_window_tokens("o3-mini"), 200_000);
    }

    #[test]
    fn test_context_window_gemini() {
        assert_eq!(context_window_tokens("gemini-2.0-flash"), 1_000_000);
    }

    #[test]
    fn test_context_window_default() {
        assert_eq!(context_window_tokens("unknown-model"), 128_000);
    }

    #[test]
    fn test_estimate_tokens_ascii() {
        // ~4 ASCII chars per token
        assert_eq!(estimate_tokens("hello world"), 2); // 11/4 = 2
        assert_eq!(estimate_tokens("a"), 1); // min 1
    }

    #[test]
    fn test_estimate_tokens_cjk() {
        // CJK: ~1.5 chars per token, should estimate higher than pure ASCII rate
        let cjk = "你好世界测试"; // 6 CJK chars
        let ascii = "abcdef"; // 6 ASCII chars = 1 token
        assert!(estimate_tokens(cjk) > estimate_tokens(ascii));
    }

    #[test]
    fn test_estimate_message_tokens() {
        let msg = Message {
            role: crew_core::MessageRole::User,
            content: "Hello, how are you today?".to_string(),
            media: vec![],
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
            timestamp: chrono::Utc::now(),
        };
        let tokens = estimate_message_tokens(&msg);
        // Should be content tokens + 4 overhead
        assert_eq!(tokens, estimate_tokens("Hello, how are you today?") + 4);
    }

    #[test]
    fn test_estimate_message_tokens_with_tool_calls() {
        let msg = Message {
            role: crew_core::MessageRole::Assistant,
            content: String::new(),
            media: vec![],
            tool_calls: Some(vec![crew_core::ToolCall {
                id: "tc1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "src/main.rs"}),
                metadata: None,
            }]),
            tool_call_id: None,
            reasoning_content: None,
            timestamp: chrono::Utc::now(),
        };
        let tokens = estimate_message_tokens(&msg);
        // Should include tool name + arguments + overhead
        assert!(tokens > 4);
        assert!(tokens > estimate_tokens("read_file"));
    }
}
