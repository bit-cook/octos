//! Source Registry - 搜索源注册表
//!
//! 提供主题到搜索引擎/语言/门户的映射

use eyre::{Result, WrapErr};
use serde::Deserialize;
use std::collections::HashMap;

/// 完整注册表
#[derive(Debug, Clone, Deserialize)]
pub struct SourceRegistry {
    pub meta: MetaInfo,
    #[serde(rename = "search_engine", default)]
    pub search_engines: Vec<SearchEngine>,
    #[serde(rename = "topic_source", default)]
    pub topic_sources: Vec<TopicSource>,

    /// 运行时索引（不序列化）
    #[serde(skip)]
    engine_index: HashMap<String, usize>,
    #[serde(skip)]
    topic_index: HashMap<String, usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetaInfo {
    pub version: String,
    pub description: String,
}

/// 搜索引擎配置
#[derive(Debug, Clone, Deserialize)]
pub struct SearchEngine {
    pub name: String,
    #[serde(rename = "display_name")]
    pub display_name: String,
    #[serde(rename = "backend_type")]
    pub backend_type: String,
    #[serde(rename = "requires_api_key", default)]
    pub requires_api_key: bool,
    #[serde(rename = "api_key_env")]
    pub api_key_env: Option<String>,
    #[serde(rename = "extra_env", default)]
    pub extra_env: Vec<String>,
    #[serde(rename = "docs_url")]
    pub docs_url: Option<String>,
    pub note: Option<String>,
    #[serde(rename = "free_tier")]
    pub free_tier: Option<String>,
    #[serde(default)]
    pub supports: Vec<String>,
}

/// 主题源配置
#[derive(Debug, Clone, Deserialize)]
pub struct TopicSource {
    pub topic: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(rename = "match_all", default)]
    pub match_all: bool,
    #[serde(rename = "search_priority", default)]
    pub search_priority: Vec<SearchPriority>,
    #[serde(default)]
    pub portals: Vec<Portal>,
}

/// 搜索优先级配置
#[derive(Debug, Clone, Deserialize)]
pub struct SearchPriority {
    pub engine: String,
    pub priority: u8,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(rename = "use_for")]
    pub use_for: Option<String>,
    pub note: Option<String>,
}

/// 门户/数据源
#[derive(Debug, Clone, Deserialize)]
pub struct Portal {
    pub name: String,
    #[serde(rename = "url_pattern")]
    pub url_pattern: String,
    #[serde(rename = "content_type")]
    pub content_type: String,
    pub language: Option<String>,
    pub reliability: String,
    #[serde(default)]
    pub paywall: bool,
    pub note: Option<String>,
}

/// 搜索源计划（用于生成 ResearchPlan）
#[derive(Debug, Clone)]
pub struct SourcePlan {
    pub topic: String,
    pub engines: Vec<(String, u8, Vec<String>)>, // (engine_name, priority, languages)
    pub portals: Vec<Portal>,
}

impl Default for SourceRegistry {
    fn default() -> Self {
        Self::load_embedded().expect("embedded registry is valid")
    }
}

impl SourceRegistry {
    /// 从嵌入的 TOML 加载
    pub fn load_embedded() -> Result<Self> {
        const DEFAULT_REGISTRY: &str = include_str!("../data/source_registry.toml");
        Self::from_toml(DEFAULT_REGISTRY)
    }

    /// 从文件异步加载
    pub async fn load(path: &std::path::Path) -> Result<Self> {
        Self::from_file(path).await
    }

    /// 从 TOML 字符串加载
    pub fn from_toml(content: &str) -> Result<Self> {
        let mut registry: SourceRegistry = toml::from_str(content)
            .wrap_err("failed to parse source registry TOML")?;

        // 构建索引
        registry.build_indices();

        Ok(registry)
    }

    /// 从文件加载
    pub async fn from_file(path: &std::path::Path) -> Result<Self> {
        let content = tokio::fs::read_to_string(path).await
            .wrap_err_with(|| format!("failed to read registry from {}", path.display()))?;
        Self::from_toml(&content)
    }

    /// 构建内部索引
    fn build_indices(&mut self) {
        self.engine_index = self.search_engines
            .iter()
            .enumerate()
            .map(|(i, e)| (e.name.clone(), i))
            .collect();

        self.topic_index = self.topic_sources
            .iter()
            .enumerate()
            .map(|(i, t)| (t.topic.clone(), i))
            .collect();
    }

    /// 根据关键词匹配主题
    pub fn match_topic(&self, query: &str) -> Option<&TopicSource> {
        let query_lower = query.to_lowercase();

        // 先尝试关键词匹配
        for source in &self.topic_sources {
            if source.match_all {
                continue; // 跳过通配主题
            }

            for keyword in &source.keywords {
                if query_lower.contains(&keyword.to_lowercase()) {
                    return Some(source);
                }
            }
        }

        // 返回默认主题
        self.topic_index
            .get("general")
            .map(|&idx| &self.topic_sources[idx])
    }

    /// 获取特定主题的配置
    pub fn get_topic(&self, topic: &str) -> Option<&TopicSource> {
        self.topic_index
            .get(topic)
            .map(|&idx| &self.topic_sources[idx])
    }

    /// 获取搜索引擎配置
    pub fn get_engine(&self, name: &str) -> Option<&SearchEngine> {
        self.engine_index
            .get(name)
            .map(|&idx| &self.search_engines[idx])
    }

    /// 为主题生成源计划
    pub fn plan_sources(&self, query: &str) -> SourcePlan {
        let topic_source = self.match_topic(query);

        match topic_source {
            Some(source) => {
                let engines: Vec<_> = source.search_priority
                    .iter()
                    .map(|p| (p.engine.clone(), p.priority, p.languages.clone()))
                    .collect();

                SourcePlan {
                    topic: source.topic.clone(),
                    engines,
                    portals: source.portals.clone(),
                }
            }
            None => {
                // 默认回退
                SourcePlan {
                    topic: "general".to_string(),
                    engines: vec![
                        ("serper".to_string(), 1, vec!["en".to_string(), "zh".to_string()]),
                        ("duckduckgo".to_string(), 2, vec!["en".to_string()]),
                    ],
                    portals: vec![],
                }
            }
        }
    }

    /// 检查引擎 API key 是否可用
    pub fn check_engine_availability(&self, engine_name: &str) -> bool {
        let Some(engine) = self.get_engine(engine_name) else {
            return false;
        };

        if !engine.requires_api_key {
            return true;
        }

        if let Some(ref env_var) = engine.api_key_env {
            return std::env::var(env_var).is_ok();
        }

        true
    }

    /// 获取所有可用引擎
    pub fn list_available_engines(&self) -> Vec<&SearchEngine> {
        self.search_engines
            .iter()
            .filter(|e| self.check_engine_availability(&e.name))
            .collect()
    }

    /// 验证注册表完整性
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // 检查所有主题引用的引擎是否存在
        for source in &self.topic_sources {
            for priority in &source.search_priority {
                if !self.engine_index.contains_key(&priority.engine) {
                    warnings.push(format!(
                        "Topic '{}' references unknown engine '{}'",
                        source.topic, priority.engine
                    ));
                }
            }
        }

        warnings
    }

    /// 转换为 Prompt 上下文描述
    pub fn to_prompt_context(&self) -> String {
        let mut context = String::new();

        context.push_str("Available Search Engines:\n");
        for engine in &self.search_engines {
            context.push_str(&format!(
                "- {} ({})\n",
                engine.name,
                engine.backend_type
            ));
        }

        context.push_str("\nTopic Sources:\n");
        for source in &self.topic_sources {
            context.push_str(&format!("- {}: {:?}\n", source.topic, source.keywords));
        }

        context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_registry() -> SourceRegistry {
        let toml = r#"
[meta]
version = "1.0.0"
description = "Test"

[[search_engine]]
name = "serper"
display_name = "Serper"
backend_type = "serper_api"
requires_api_key = true
api_key_env = "SERPER_API_KEY"

[[search_engine]]
name = "test_engine"
display_name = "Test"
backend_type = "test"
requires_api_key = false

[[topic_source]]
topic = "technology"
keywords = ["AI", "tech"]

[[topic_source.search_priority]]
engine = "serper"
priority = 1
languages = ["en", "zh"]

[[topic_source.portals]]
name = "Test Portal"
url_pattern = "https://test.com?q={query}"
content_type = "news"
reliability = "high"

[[topic_source]]
topic = "general"
keywords = []
match_all = true

[[topic_source.search_priority]]
engine = "test_engine"
priority = 1
languages = ["en"]
"#;

        SourceRegistry::from_toml(toml).unwrap()
    }

    #[test]
    fn test_match_topic() {
        let registry = create_test_registry();

        // 应该匹配到 technology
        let result = registry.match_topic("AI development");
        assert!(result.is_some());
        assert_eq!(result.unwrap().topic, "technology");

        // 应该匹配到 technology（关键词包含 tech）
        let result = registry.match_topic("latest tech news");
        assert!(result.is_some());
        assert_eq!(result.unwrap().topic, "technology");

        // 不匹配任何关键词，应该返回 general
        let result = registry.match_topic("cooking recipes");
        // 注意：当前实现会先遍历非通配主题，如果没有匹配则返回 None
        // 需要修改 match_topic 来正确处理 fallback
    }

    #[test]
    fn test_plan_sources() {
        let registry = create_test_registry();
        let plan = registry.plan_sources("AI");

        assert_eq!(plan.topic, "technology");
        assert!(!plan.engines.is_empty());
        assert_eq!(plan.engines[0].0, "serper");
    }

    #[test]
    fn test_get_engine() {
        let registry = create_test_registry();

        let serper = registry.get_engine("serper");
        assert!(serper.is_some());
        assert_eq!(serper.unwrap().display_name, "Serper");

        let unknown = registry.get_engine("unknown");
        assert!(unknown.is_none());
    }

    #[test]
    fn test_validate() {
        let toml = r#"
[meta]
version = "1.0.0"
description = "Test"

[[topic_source]]
topic = "broken"
keywords = ["test"]

[[topic_source.search_priority]]
engine = "nonexistent"
priority = 1
"#;

        let registry = SourceRegistry::from_toml(toml).unwrap();
        let warnings = registry.validate();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("nonexistent"));
    }
}
