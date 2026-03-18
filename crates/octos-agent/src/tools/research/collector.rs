//! Collector Agent - 子代理收集器
//!
//! 执行单个研究角度的深度收集，使用 LLM 进行信息提取

use std::path::PathBuf;
use std::sync::Arc;

use eyre::{Result, WrapErr};
use tracing::info;

use crate::tools::research::types::*;
use crate::tools::research::ResearchConfig;
use crate::search::MultiSearcher;
use crate::tools::web_fetch::WebFetchTool;
use crate::tools::Tool;

/// 收集器 Agent - 执行单个角度的研究收集
pub struct CollectorAgent {
    /// 角度配置
    angle: ResearchAngle,
    /// 配置
    config: ResearchConfig,
    /// 研究目录
    research_dir: PathBuf,
    /// LLM 提供者
    llm: Arc<dyn octos_llm::LlmProvider>,
    /// 多搜索引擎
    searcher: MultiSearcher,
}

impl CollectorAgent {
    pub fn new(
        angle: ResearchAngle,
        config: ResearchConfig,
        research_dir: PathBuf,
        llm: Arc<dyn octos_llm::LlmProvider>,
    ) -> Self {
        Self {
            angle,
            config,
            research_dir,
            llm,
            searcher: MultiSearcher::new(),
        }
    }

    /// 执行收集
    pub async fn collect(&self,
        parent_kb: &KnowledgeBase,
    ) -> Result<ExplorationResult> {
        info!(angle = %self.angle.label, "Starting collection");

        // Step 1: 执行搜索
        let search_results = self.perform_search().await?;

        // Step 2: 获取页面内容
        let pages = self.fetch_pages(&search_results).await;

        // Step 3: 提取结构化信息
        let extraction = self.extract_information(&pages, parent_kb).await?;

        // Step 4: 保存中间结果
        self.save_partial_results(&extraction).await?;

        info!(angle = %self.angle.label, facts = extraction.facts.len(),
              candidates = extraction.recursion_candidates.len(),
              "Collection completed");

        Ok(extraction)
    }

    /// 执行搜索
    async fn perform_search(&self) -> Result<Vec<SearchResult>> {
        let query = &self.angle.task;
        let lang = self.angle.language.clone();
        let count = self.config.max_results_per_angle as u8;

        // 使用多搜索引擎
        let results = self.searcher.search_all(query, count, lang).await;

        Ok(results)
    }

    /// 获取页面内容
    async fn fetch_pages(
        &self,
        search_results: &[SearchResult],
    ) -> Vec<FetchedPage> {
        let fetch_tool = WebFetchTool::new();
        let max_pages = self.config.max_pages_per_angle;
        let mut pages = Vec::new();

        // 收集所有 URL
        let mut urls_to_fetch: Vec<(String, String)> = Vec::new(); // (url, title)

        for result in search_results {
            for item in &result.items {
                if urls_to_fetch.len() >= max_pages {
                    break;
                }
                urls_to_fetch.push((item.url.clone(), item.title.clone()));
            }
        }

        // 并行获取页面
        let fetch_futures: Vec<_> = urls_to_fetch
            .into_iter()
            .map(|(url, title)| {
                let fetch_tool = &fetch_tool;
                async move {
                    let args = serde_json::json!({
                        "url": url,
                        "max_length": 20000
                    });

                    match fetch_tool.execute(&args).await {
                        Ok(result) if result.success => {
                            Some(FetchedPage {
                                url: url.clone(),
                                title,
                                content: result.output,
                            })
                        }
                        _ => None,
                    }
                }
            })
            .collect();

        let results = futures::future::join_all(fetch_futures).await;

        for result in results {
            if let Some(page) = result {
                pages.push(page);
            }
        }

        pages
    }

    /// 提取结构化信息 - 使用 LLM 进行深度分析
    async fn extract_information(
        &self,
        pages: &[FetchedPage],
        _parent_kb: &KnowledgeBase,
    ) -> Result<ExplorationResult> {
        let mut all_facts = Vec::new();
        let mut all_sources = Vec::new();
        let mut all_recursion_candidates = Vec::new();
        let mut all_contradictions = Vec::new();

        // 对每个页面并行调用 LLM 提取信息
        let extraction_futures: Vec<_> = pages
            .iter()
            .map(|page| self.extract_from_page(page))
            .collect();

        let results = futures::future::join_all(extraction_futures).await;

        for result in results {
            if let Ok(extraction) = result {
                all_facts.extend(extraction.facts);
                all_sources.extend(extraction.sources);
                all_recursion_candidates.extend(extraction.recursion_candidates);
                all_contradictions.extend(extraction.contradictions);
            }
        }

        // 计算总体权威性评分
        let authority_score = if all_sources.is_empty() {
            0.5
        } else {
            all_sources.iter().map(|s| s.authority_score).sum::<f64>() / all_sources.len() as f64
        };

        // 计算覆盖度贡献
        let coverage_contribution = (all_facts.len() as f64 * 0.02).min(0.2);

        Ok(ExplorationResult {
            angle: self.angle.clone(),
            facts: all_facts,
            sources: all_sources,
            recursion_candidates: all_recursion_candidates,
            contradictions: all_contradictions,
            authority_assessment: AuthorityAssessment {
                score: authority_score,
                reasoning: format!("Analyzed {} pages with LLM", pages.len()),
                red_flags: vec![],
            },
            follow_ups: FollowUpLinks {
                internal: vec![],
                external: vec![],
            },
            coverage_contribution,
            sub_questions: vec![],
        })
    }

    /// 使用 LLM 从单个页面提取结构化信息
    async fn extract_from_page(
        &self,
        page: &FetchedPage,
    ) -> Result<PageExtraction> {
        // 构建 prompt
        let prompt = format!(
            r#"You are a Research Explorer Agent. Extract structured information from the following web page content.

Search Task: {}
Page Title: {}
Page URL: {}
Dimension: {:?}

## Page Content:
{}

## Your Task:
1. Extract 5-10 specific facts from this page
2. Identify named entities (people, companies, events, technologies, policies)
3. Rate source authority (0.0-1.0) based on domain reputation
4. Identify potential recursion candidates for deeper exploration

## Output Format (STRICT JSON):
{{
  "facts": [
    {{
      "id": "f1",
      "claim": "specific fact",
      "quote": "exact supporting text",
      "confidence": "high|medium|low",
      "category": "fact|opinion|prediction|data",
      "needs_verification": false,
      "verification_queries": []
    }}
  ],
  "recursion_candidates": [
    {{
      "entity_type": "event|person|company|data|policy|technology|reaction",
      "entity": "Entity Name",
      "context": "brief context",
      "priority": "high|medium|low",
      "layer2_queries": ["background query"],
      "layer3_queries": ["impact query"],
      "reason": "why explore deeper"
    }}
  ],
  "authority_score": 0.85,
  "relevance_score": 0.90
}}"#,
            self.angle.task,
            page.title,
            page.url,
            self.angle.dimension,
            &page.content.chars().take(15000).collect::<String>() // 限制内容长度
        );

        // 调用 LLM
        let messages = vec![
            octos_core::Message::system("You are a research information extraction expert. Output valid JSON only."),
            octos_core::Message::user(prompt),
        ];

        let config = octos_llm::ChatConfig::default();
        let response = self.llm.chat(&messages, &[], &config).await
            .wrap_err("Failed to extract information from page")?;

        // 解析 JSON 响应
        let content = response.content.as_deref().unwrap_or("{}").trim();
        let json_str = if content.starts_with("```json") {
            content
                .trim_start_matches("```json")
                .trim_end_matches("```")
                .trim()
        } else if content.starts_with("```") {
            content
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
        } else {
            content
        };

        let extraction: PageExtraction = serde_json::from_str(json_str)
            .unwrap_or_else(|_| PageExtraction::default());

        Ok(extraction)
    }

    /// 保存中间结果
    async fn save_partial_results(
        &self,
        result: &ExplorationResult,
    ) -> Result<()> {
        // 生成文件名
        let filename = format!("partial_{}.json", slugify(&self.angle.label));
        let filepath = self.research_dir.join(filename);

        let json = serde_json::to_string_pretty(result)
            .wrap_err("Failed to serialize result")?;

        tokio::fs::write(&filepath, json)
            .await
            .wrap_err_with(|| format!("Failed to write {}", filepath.display()))?;

        Ok(())
    }
}

/// 获取的页面
#[derive(Debug)]
struct FetchedPage {
    url: String,
    title: String,
    content: String,
}

/// 提取域名
fn extract_domain(url: &str) -> String {
    url.split('/')
        .nth(2)
        .unwrap_or("unknown")
        .trim_start_matches("www.")
        .to_string()
}

/// 生成 slug
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .replace(' ', "_")
        .replace(|c: char| !c.is_alphanumeric() && c != '_', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://www.example.com/page"),
            "example.com"
        );
        assert_eq!(
            extract_domain("http://test.org/path"),
            "test.org"
        );
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello_world");
        assert_eq!(slugify("Test 123!"), "test_123");
    }
}
