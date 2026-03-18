//! Baidu Backend - 百度搜索（HTML 爬取）
//!
//! 注意：需要处理反爬机制，可能需要代理

use async_trait::async_trait;
use eyre::{Result, WrapErr};
use reqwest::Client;
use scraper::{Html, Selector};

use crate::search::SearchBackend;
use crate::tools::research::types::{Language, SearchResult, SearchResultItem};

const BAIDU_SEARCH_URL: &str = "https://www.baidu.com/s";

pub struct BaiduBackend {
    client: Client,
}

impl BaiduBackend {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// 构建百度搜索 URL
    fn build_url(&self, query: &str, count: u8) -> String {
        let encoded = urlencoding::encode(query);
        // Baidu 的 pn 参数是偏移量，每页 10 条
        let num = count.clamp(1, 50);
        format!("{}?wd={}&rn={}", BAIDU_SEARCH_URL, encoded, num)
    }

    /// 解析百度结果 HTML
    fn parse_results(&self, html: &str) -> Vec<SearchResultItem> {
        let document = Html::parse_document(html);
        let mut items = Vec::new();

        // 尝试多个可能的结果容器选择器
        let selectors = [
            // 标准结果
            "#content_left .result",
            // 其他可能的选择器
            ".c-container",
        ];

        for selector_str in &selectors {
            if let Ok(selector) = Selector::parse(selector_str) {
                for element in document.select(&selector) {
                    if let Some(item) = self.parse_result_item(&element) {
                        items.push(item);
                    }
                }

                if !items.is_empty() {
                    break; // 找到结果就停止
                }
            }
        }

        items
    }

    fn parse_result_item(
        &self,
        element: &scraper::ElementRef,
    ) -> Option<SearchResultItem> {
        // 提取标题
        let title_sel = Selector::parse("h3, .t").ok()?;
        let title = element
            .select(&title_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())?;

        if title.is_empty() {
            return None;
        }

        // 提取 URL
        let link_sel = Selector::parse("a[href]").ok()?;
        let url = element
            .select(&link_sel)
            .next()
            .and_then(|e| e.value().attr("href"))
            .map(|s| s.to_string())
            .unwrap_or_default();

        // 提取摘要
        let snippet_sel = Selector::parse(".content-right_8Zs40, .c-abstract, .content").ok()?;
        let snippet = element
            .select(&snippet_sel)
            .next()
            .map(|e| {
                e.text()
                    .collect::<String>()
                    .trim()
                    .replace("\n", " ")
                    .to_string()
            })
            .unwrap_or_default();

        Some(SearchResultItem {
            title,
            url: self.resolve_url(&url),
            snippet,
            published_date: None,
        })
    }

    /// 解析百度重定向 URL
    fn resolve_url(&self, url: &str) -> String {
        // 百度结果是重定向链接，需要解析
        // 格式: https://www.baidu.com/link?url=xxx
        if url.starts_with("http") {
            return url.to_string();
        }
        if url.starts_with("/link") {
            return format!("https://www.baidu.com{}", url);
        }
        url.to_string()
    }
}

#[async_trait]
impl SearchBackend for BaiduBackend {
    fn name(&self) -> &str {
        "baidu"
    }

    fn is_available(&self) -> bool {
        // 百度不需要 API key，总是可用
        true
    }

    async fn search(
        &self,
        query: &str,
        count: u8,
        language: Language,
    ) -> Result<SearchResult> {
        // 百度主要支持中文
        if language != Language::Zh {
            tracing::debug!("Baidu works best with Chinese queries");
        }

        let url = self.build_url(query, count);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .wrap_err("failed to send request to Baidu")?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(eyre::eyre!("Baidu returned error status: {}", status));
        }

        let html = response
            .text()
            .await
            .wrap_err("failed to read Baidu response")?;

        // 检查是否有验证码
        if html.contains("验证码") || html.contains("security") {
            return Err(eyre::eyre!(
                "Baidu requires CAPTCHA - may need proxy or rate limit handling"
            ));
        }

        let items = self.parse_results(&html);

        Ok(SearchResult {
            query: query.to_string(),
            items,
            engine: "baidu".to_string(),
            language,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baidu_url_building() {
        let baidu = BaiduBackend::new();
        let url = baidu.build_url("人工智能", 10);
        assert!(url.contains("wd=%E4%BA%BA%E5%B7%A5%E6%99%BA%E8%83%BD"));
        assert!(url.contains("rn=10"));
    }

    #[test]
    fn test_parse_sample_html() {
        // 简化的测试 HTML
        let html = r#"
        <html>
        <body>
        <div id="content_left">
            <div class="result">
                <h3><a href="/link?url=test1">Test Title 1</a></h3>
                <div class="c-abstract">Test snippet 1</div>
            </div>
            <div class="result">
                <h3><a href="/link?url=test2">Test Title 2</a></h3>
                <div class="c-abstract">Test snippet 2</div>
            </div>
        </div>
        </body>
        </html>
        "#;

        let baidu = BaiduBackend::new();
        let items = baidu.parse_results(html);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Test Title 1");
        assert!(items[0].url.contains("baidu.com"));
    }

    #[test]
    fn test_resolve_url() {
        let baidu = BaiduBackend::new();

        assert_eq!(
            baidu.resolve_url("/link?url=abc"),
            "https://www.baidu.com/link?url=abc"
        );
        assert_eq!(
            baidu.resolve_url("https://example.com"),
            "https://example.com"
        );
    }
}
