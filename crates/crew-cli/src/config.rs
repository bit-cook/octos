//! Configuration file support for crew CLI.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// LLM provider: "anthropic", "openai", or "gemini".
    #[serde(default)]
    pub provider: Option<String>,

    /// Model name.
    #[serde(default)]
    pub model: Option<String>,

    /// Custom base URL for the API endpoint.
    #[serde(default)]
    pub base_url: Option<String>,

    /// Environment variable name for API key (default: ANTHROPIC_API_KEY, OPENAI_API_KEY, or GEMINI_API_KEY).
    #[serde(default)]
    pub api_key_env: Option<String>,

    /// Gateway configuration (optional).
    #[serde(default)]
    pub gateway: Option<GatewayConfig>,
}

/// Gateway mode configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Channels to enable.
    #[serde(default)]
    pub channels: Vec<ChannelEntry>,

    /// Maximum conversation history messages to include.
    #[serde(default = "default_max_history")]
    pub max_history: usize,

    /// Custom system prompt for gateway mode.
    #[serde(default)]
    pub system_prompt: Option<String>,
}

/// A channel entry in gateway config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEntry {
    /// Channel type: "cli", "telegram", "discord".
    #[serde(rename = "type")]
    pub channel_type: String,

    /// Allowed sender IDs (empty = allow all).
    #[serde(default)]
    pub allowed_senders: Vec<String>,

    /// Channel-specific settings.
    #[serde(default)]
    pub settings: serde_json::Value,
}

fn default_max_history() -> usize {
    50
}

impl Config {
    /// Load config from file, returns default if not found.
    pub fn load(cwd: &Path) -> Result<Self> {
        // Try project-local config first
        let local_config = cwd.join(".crew").join("config.json");
        if local_config.exists() {
            return Self::from_file(&local_config);
        }

        // Try global config
        if let Some(global_config) = Self::global_config_path() {
            if global_config.exists() {
                return Self::from_file(&global_config);
            }
        }

        // No config found, use defaults
        Ok(Self::default())
    }

    /// Load config from a specific file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("failed to read config file: {}", path.display()))?;
        let mut config: Self = serde_json::from_str(&content)
            .wrap_err_with(|| format!("failed to parse config file: {}", path.display()))?;

        // Expand environment variables in config values
        config.expand_env_vars();
        Ok(config)
    }

    /// Get global config path (~/.config/crew/config.json).
    pub fn global_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("crew").join("config.json"))
    }

    /// Expand environment variables in config values.
    /// Supports ${VAR_NAME} syntax.
    fn expand_env_vars(&mut self) {
        if let Some(ref mut base_url) = self.base_url {
            *base_url = Self::expand_env_var(base_url);
        }
        if let Some(ref mut model) = self.model {
            *model = Self::expand_env_var(model);
        }
        if let Some(ref mut provider) = self.provider {
            *provider = Self::expand_env_var(provider);
        }
    }

    /// Expand ${VAR_NAME} patterns in a string.
    fn expand_env_var(s: &str) -> String {
        let mut result = s.to_string();
        let mut start = 0;

        while let Some(begin) = result[start..].find("${") {
            let begin = start + begin;
            if let Some(end) = result[begin..].find('}') {
                let end = begin + end;
                let var_name = &result[begin + 2..end];
                if let Ok(value) = std::env::var(var_name) {
                    result = format!("{}{}{}", &result[..begin], value, &result[end + 1..]);
                    start = begin + value.len();
                } else {
                    start = end + 1;
                }
            } else {
                break;
            }
        }
        result
    }

    /// Get the API key from environment, using configured env var name or default.
    pub fn get_api_key(&self, provider: &str) -> Result<String> {
        let env_var = self.api_key_env.clone().unwrap_or_else(|| match provider {
            "anthropic" => "ANTHROPIC_API_KEY".to_string(),
            "openai" => "OPENAI_API_KEY".to_string(),
            "gemini" => "GEMINI_API_KEY".to_string(),
            _ => format!("{}_API_KEY", provider.to_uppercase()),
        });

        std::env::var(&env_var)
            .wrap_err_with(|| format!("{} environment variable not set", env_var))
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<Vec<String>> {
        let mut warnings = Vec::new();

        // Check provider is valid
        if let Some(ref provider) = self.provider {
            if !["anthropic", "openai", "gemini"].contains(&provider.as_str()) {
                warnings.push(format!(
                    "Unknown provider '{}'. Valid options: anthropic, openai, gemini",
                    provider
                ));
            }
        }

        // Check API key is set
        let provider = self.provider.as_deref().unwrap_or("anthropic");
        if self.get_api_key(provider).is_err() {
            let env_var = self.api_key_env.clone().unwrap_or_else(|| match provider {
                "anthropic" => "ANTHROPIC_API_KEY".to_string(),
                "openai" => "OPENAI_API_KEY".to_string(),
                "gemini" => "GEMINI_API_KEY".to_string(),
                _ => format!("{}_API_KEY", provider.to_uppercase()),
            });
            warnings.push(format!("{} environment variable not set", env_var));
        }

        Ok(warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_var() {
        // SAFETY: This is a test running in isolation
        unsafe {
            std::env::set_var("TEST_VAR", "hello");
        }
        assert_eq!(Config::expand_env_var("${TEST_VAR}"), "hello");
        assert_eq!(
            Config::expand_env_var("prefix_${TEST_VAR}_suffix"),
            "prefix_hello_suffix"
        );
        assert_eq!(Config::expand_env_var("no_var"), "no_var");
        assert_eq!(
            Config::expand_env_var("${UNDEFINED_VAR}"),
            "${UNDEFINED_VAR}"
        );
        // SAFETY: This is a test running in isolation
        unsafe {
            std::env::remove_var("TEST_VAR");
        }
    }

    #[test]
    fn test_gateway_config_deserialize() {
        let json = r#"{
            "provider": "anthropic",
            "model": "claude-sonnet-4-20250514",
            "gateway": {
                "channels": [{"type": "cli"}],
                "max_history": 30
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let gw = config.gateway.unwrap();
        assert_eq!(gw.channels.len(), 1);
        assert_eq!(gw.channels[0].channel_type, "cli");
        assert_eq!(gw.max_history, 30);
        assert!(gw.system_prompt.is_none());
    }

    #[test]
    fn test_gateway_config_defaults() {
        let json = r#"{"provider": "anthropic"}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.gateway.is_none());
    }

    #[test]
    fn test_gateway_max_history_default() {
        let json = r#"{"channels": [{"type": "cli"}]}"#;
        let gw: GatewayConfig = serde_json::from_str(json).unwrap();
        assert_eq!(gw.max_history, 50);
    }

    #[test]
    fn test_validate_unknown_provider() {
        let config = Config {
            provider: Some("invalid".to_string()),
            ..Default::default()
        };
        let warnings = config.validate().unwrap();
        assert!(warnings.iter().any(|w| w.contains("Unknown provider")));
    }
}
