//! Search Backends - 多搜索引擎支持
//!
//! 提供统一的 SearchBackend trait，支持多种搜索引擎并行查询

use async_trait::async_trait;
use eyre::Result;

use crate::tools::research::types::{SearchResult, SearchResultItem, Language};

mod serper;
mod baidu;

pub use serper::SerperBackend;
pub use baidu::BaiduBackend;

/// 搜索引擎后端 trait
#[async_trait]
pub trait SearchBackend: Send + Sync {
    /// 后端名称
    fn name(&self) -> &str;

    /// 是否可用（API key 是否配置）
    fn is_available(&self) -> bool;

    /// 执行搜索
    ///
    /// # Arguments
    /// * `query` - 搜索查询
    /// * `count` - 返回结果数
    /// * `language` - 语言
    ///
    /// # Returns
    /// 搜索结果
    async fn search(
        &self,
        query: &str,
        count: u8,
        language: Language,
    ) -> Result<SearchResult>;
}

/// 多搜索引擎（并行查询 + 去重）
pub struct MultiSearcher {
    backends: Vec<Box<dyn SearchBackend>>,
}

impl MultiSearcher {
    /// 创建新的多搜索器
    pub fn new() -> Self {
        let mut backends: Vec<Box<dyn SearchBackend>> = Vec::new();

        // 尝试添加 Serper（推荐主引擎）
        let serper = SerperBackend::new();
        if serper.is_available() {
            backends.push(Box::new(serper));
        }

        // 尝试添加百度
        let baidu = BaiduBackend::new();
        backends.push(Box::new(baidu));

        Self { backends }
    }

    /// 从配置创建
    pub fn from_backends(backends: Vec<Box<dyn SearchBackend>>) -> Self {
        Self { backends }
    }

    /// 并行搜索所有可用后端
    pub async fn search_all(
        &self,
        query: &str,
        count: u8,
        language: Language,
    ) -> Vec<SearchResult> {
        use futures::future::join_all;

        let futures: Vec<_> = self
            .backends
            .iter()
            .filter(|b| b.is_available())
            .map(|backend| async move {
                let name = backend.name().to_string();
                match backend.search(query, count, language).await {
                    Ok(result) => {
                        tracing::info!(backend = %name, results = result.items.len(), "search completed");
                        Some(result)
                    }
                    Err(e) => {
                        tracing::warn!(backend = %name, error = %e, "search failed");
                        None
                    }
                }
            })
            .collect();

        join_all(futures)
            .await
            .into_iter()
            .flatten()
            .collect()
    }

    /// 搜索并合并去重
    pub async fn search_merged(
        &self,
        query: &str,
        count: u8,
        language: Language,
    ) -> SearchResult {
        let results = self.search_all(query, count, language).await;

        let mut merged_items: Vec<SearchResultItem> = Vec::new();
        let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();

        for result in results {
            for item in result.items {
                let normalized_url = normalize_url(&item.url);
                if seen_urls.insert(normalized_url) {
                    merged_items.push(item);
                }
            }
        }

        // 按相关性排序（简单实现：保留原顺序）
        merged_items.truncate(count as usize);

        SearchResult {
            query: query.to_string(),
            items: merged_items,
            engine: "multi".to_string(),
            language,
        }
    }

    /// 获取可用后端列表
    pub fn available_backends(&self) -> Vec<&str> {
        self.backends
            .iter()
            .filter(|b| b.is_available())
            .map(|b| b.name())
            .collect()
    }
}

impl Default for MultiSearcher {
    fn default() -> Self {
        Self::new()
    }
}

/// URL 归一化（用于去重）
fn normalize_url(url: &str) -> String {
    url.to_lowercase()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_string()
}

/// HTML 内容提取（共享工具函数）
pub mod extract {
    use scraper::{Html, Selector};

    /// 从 HTML 提取标题
    pub fn extract_title(html: &str) -> Option<String> {
        let document = Html::parse_document(html);
        let selector = Selector::parse("title").ok()?;
        document
            .select(&selector)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
    }

    /// 从 HTML 提取主要文本内容
    pub fn extract_content(html: &str, max_chars: usize) -> String {
        let document = Html::parse_document(html);

        // 尝试常见的内容选择器
        let selectors = [
            "article",
            "main",
            "[role='main']",
            ".content",
            "#content",
            ".post",
            ".entry",
            "body",
        ];

        for sel_str in &selectors {
            if let Ok(selector) = Selector::parse(sel_str) {
                if let Some(elem) = document.select(&selector).next() {
                    let text = elem.text().collect::<String>();
                    let cleaned = clean_text(&text);
                    if cleaned.len() > 100 {
                        return truncate(&cleaned, max_chars);
                    }
                }
            }
        }

        // 回退：提取 body 文本
        let body_selector = Selector::parse("body").unwrap();
        if let Some(body) = document.select(&body_selector).next() {
            let text = body.text().collect::<String>();
            return truncate(&clean_text(&text), max_chars);
        }

        String::new()
    }

    /// 从 HTML 提取元数据
    pub fn extract_metadata(html: &str) -> Metadata {
        let document = Html::parse_document(html);

        Metadata {
            title: extract_meta(&document, "og:title")
                .or_else(|| extract_meta(&document, "twitter:title"))
                .or_else(|| {
                    let sel = Selector::parse("title").ok()?;
                    document.select(&sel).next().map(|e| e.text().collect())
                }),
            description: extract_meta(&document, "og:description")
                .or_else(|| extract_meta(&document, "twitter:description"))
                .or_else(|| extract_meta(&document, "description")),
            author: extract_meta(&document, "author")
                .or_else(|| extract_meta(&document, "og:author")),
            published_date: extract_meta(&document, "article:published_time")
                .or_else(|| extract_meta(&document, "datePublished")),
        }
    }

    fn extract_meta(document: &Html, property: &str) -> Option<String> {
        // Try meta property
        let sel = format!("meta[property='{}']", property);
        if let Ok(selector) = Selector::parse(&sel) {
            if let Some(elem) = document.select(&selector).next() {
                if let Some(content) = elem.value().attr("content") {
                    return Some(content.to_string());
                }
            }
        }

        // Try meta name
        let sel = format!("meta[name='{}']", property);
        if let Ok(selector) = Selector::parse(&sel) {
            if let Some(elem) = document.select(&selector).next() {
                if let Some(content) = elem.value().attr("content") {
                    return Some(content.to_string());
                }
            }
        }

        None
    }

    fn clean_text(text: &str) -> String {
        text.lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn truncate(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            s.to_string()
        } else {
            s.chars().take(max_chars).collect::<String>() + "..."
        }
    }

    #[derive(Debug, Default)]
    pub struct Metadata {
        pub title: Option<String>,
        pub description: Option<String>,
        pub author: Option<String>,
        pub published_date: Option<String>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url("https://Example.COM/Page"),
            "example.com/page"
        );
        assert_eq!(
            normalize_url("http://www.example.com/page/"),
            "example.com/page"
        );
        assert_eq!(
            normalize_url("https://example.com/page"),
            "example.com/page"
        );
    }

    #[test]
    fn test_extract_title() {
        let html = r#"<html><head><title>Test Page</title></head><body></body></html>"#;
        assert_eq!(extract::extract_title(html), Some("Test Page".to_string()));
    }

    #[test]
    fn test_extract_metadata() {
        let html = r#"
            <html>
            <head>
                <meta property="og:title" content="OG Title">
                <meta name="description" content="Page description">
                <meta property="article:published_time" content="2026-03-18">
            </head>
            </html>
        "#;
        let meta = extract::extract_metadata(html);
        assert_eq!(meta.title, Some("OG Title".to_string()));
        assert_eq!(meta.description, Some("Page description".to_string()));
        assert_eq!(meta.published_date, Some("2026-03-18".to_string()));
    }
}
