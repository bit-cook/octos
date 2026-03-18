//! Serper.dev Backend - Google Search via API
//!
//! 推荐主引擎：$50/month for 50K queries
//! 支持：Web, News, Images, Places
//! 还支持 site:x.com 代理搜索社交媒体

use async_trait::async_trait;
use eyre::{Result, WrapErr};
use reqwest::Client;
use serde::Deserialize;

use crate::search::SearchBackend;
use crate::tools::research::types::{Language, SearchResult, SearchResultItem};

const SERPER_API_URL: &str = "https://google.serper.dev/search";
const SERPER_NEWS_URL: &str = "https://google.serper.dev/news";

pub struct SerperBackend {
    client: Client,
    api_key: Option<String>,
}

impl SerperBackend {
    pub fn new() -> Self {
        let api_key = std::env::var("SERPER_API_KEY").ok();
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
            api_key,
        }
    }

    /// 搜索新闻（专门端点）
    pub async fn search_news(
        &self,
        query: &str,
        count: u8,
        language: Language,
    ) -> Result<SearchResult> {
        self.search_with_endpoint(query, count, language, SERPER_NEWS_URL, "news")
            .await
    }

    /// 搜索社交媒体（使用 site: 操作符）
    pub async fn search_social(
        &self,
        query: &str,
        platform: &str, // x, reddit, linkedin
        count: u8,
        language: Language,
    ) -> Result<SearchResult> {
        let site_query = format!("{} site:{}.com", query, platform);
        self.search_with_endpoint(&site_query, count, language, SERPER_API_URL, "web")
            .await
    }

    async fn search_with_endpoint(
        &self,
        query: &str,
        count: u8,
        language: Language,
        endpoint: &str,
        search_type: &str,
    ) -> Result<SearchResult> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| eyre::eyre!("SERPER_API_KEY not set"))?;

        let num = count.clamp(1, 100);
        let hl = language_to_serper_lang(language);
        let gl = language_to_serper_region(language);

        let request_body = serde_json::json!({
            "q": query,
            "num": num,
            "hl": hl,
            "gl": gl,
            "autocorrect": false,
        });

        let response = self
            .client
            .post(endpoint)
            .header("X-API-KEY", api_key)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .wrap_err("failed to send request to Serper")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(eyre::eyre!("Serper API error: {} - {}", status, text));
        }

        let serper_resp: SerperResponse = response
            .json()
            .await
            .wrap_err("failed to parse Serper response")?;

        let items = match search_type {
            "news" => serper_resp
                .news
                .into_iter()
                .flatten()
                .map(|n| SearchResultItem {
                    title: n.title,
                    url: n.link,
                    snippet: n.snippet.unwrap_or_default(),
                    published_date: n.date,
                })
                .collect(),
            _ => serper_resp
                .organic
                .into_iter()
                .flatten()
                .map(|o| SearchResultItem {
                    title: o.title,
                    url: o.link,
                    snippet: o.snippet.unwrap_or_default(),
                    published_date: None,
                })
                .collect(),
        };

        Ok(SearchResult {
            query: query.to_string(),
            items,
            engine: format!("serper:{}", search_type),
            language,
        })
    }
}

#[async_trait]
impl SearchBackend for SerperBackend {
    fn name(&self) -> &str {
        "serper"
    }

    fn is_available(&self) -> bool {
        self.api_key.is_some()
    }

    async fn search(
        &self,
        query: &str,
        count: u8,
        language: Language,
    ) -> Result<SearchResult> {
        self.search_with_endpoint(query, count, language, SERPER_API_URL, "web")
            .await
    }
}

/// Serper API 响应结构
#[derive(Debug, Deserialize)]
struct SerperResponse {
    #[serde(default)]
    organic: Option<Vec<SerperOrganicResult>>,
    #[serde(default)]
    news: Option<Vec<SerperNewsResult>>,
    #[allow(dead_code)]
    #[serde(default)]
    knowledge_graph: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct SerperOrganicResult {
    title: String,
    link: String,
    #[serde(default)]
    snippet: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SerperNewsResult {
    title: String,
    link: String,
    #[serde(default)]
    snippet: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    source: Option<String>,
}

/// 语言代码转换
fn language_to_serper_lang(lang: Language) -> &'static str {
    match lang {
        Language::En => "en",
        Language::Zh => "zh",
        Language::Es => "es",
        Language::Ja => "ja",
        Language::Ko => "ko",
        Language::De => "de",
        Language::Fr => "fr",
    }
}

/// 地区代码转换
fn language_to_serper_region(lang: Language) -> &'static str {
    match lang {
        Language::En => "us",
        Language::Zh => "cn",
        Language::Es => "es",
        Language::Ja => "jp",
        Language::Ko => "kr",
        Language::De => "de",
        Language::Fr => "fr",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serper_availability() {
        // 没有 API key 时不可用
        let backend = SerperBackend {
            client: Client::new(),
            api_key: None,
        };
        assert!(!backend.is_available());

        // 有 API key 时可用
        let backend = SerperBackend {
            client: Client::new(),
            api_key: Some("test-key".to_string()),
        };
        assert!(backend.is_available());
    }

    #[test]
    fn test_language_conversion() {
        assert_eq!(language_to_serper_lang(Language::En), "en");
        assert_eq!(language_to_serper_lang(Language::Zh), "zh");
        assert_eq!(language_to_serper_region(Language::Zh), "cn");
    }
}
