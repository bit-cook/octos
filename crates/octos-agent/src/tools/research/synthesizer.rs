//! Synthesizer - 报告合成器
//!
//! 整合所有探索结果，生成最终研究报告

use std::path::PathBuf;
use std::sync::Arc;

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::tools::research::types::*;

/// LLM 生成的报告结构
#[derive(Debug, Clone, Deserialize, Serialize)]
struct LlmGeneratedReport {
    executive_summary: String,
    key_findings: Vec<KeyFinding>,
    detailed_analysis: String,
    contradictions: Vec<ContradictionSection>,
    gaps: Vec<GapSection>,
    methodology: String,
}

/// 报告合成器
pub struct Synthesizer {
    research_dir: PathBuf,
    config: ResearchConfig,
    llm: Arc<dyn octos_llm::LlmProvider>,
}

impl Synthesizer {
    pub fn new(research_dir: PathBuf, config: ResearchConfig, llm: Arc<dyn octos_llm::LlmProvider>) -> Self {
        Self {
            research_dir,
            config,
            llm,
        }
    }

    /// 合成最终报告 - 使用 LLM 进行深度分析
    pub async fn synthesize(
        &self,
        query: &str,
        kb: &KnowledgeBase,
        partial_results: &[ExplorationResult],
    ) -> Result<ResearchReport> {
        info!("Starting synthesis with LLM");

        let started_at = chrono::Utc::now().to_rfc3339();

        // 使用 LLM 生成完整报告
        let llm_report = self.generate_llm_report(query, kb, partial_results).await?;

        let completed_at = chrono::Utc::now().to_rfc3339();

        let report = ResearchReport {
            query: query.to_string(),
            executive_summary: llm_report.executive_summary,
            key_findings: llm_report.key_findings,
            detailed_analysis: llm_report.detailed_analysis,
            contradictions: llm_report.contradictions,
            gaps: llm_report.gaps,
            sources: self.compile_sources(kb).await?,
            metadata: ReportMetadata {
                started_at,
                completed_at,
                angles_count: partial_results.len(),
                facts_count: kb.facts.len(),
                sources_count: kb.sources.len(),
                recursion_depth: kb.current_layer,
                estimated_tokens: self.estimate_tokens(kb),
            },
            methodology: llm_report.methodology,
        };

        // 保存报告
        self.save_report(&report).await?;

        info!("Synthesis completed");
        Ok(report)
    }

    /// 使用 LLM 生成完整报告
    async fn generate_llm_report(
        &self,
        query: &str,
        kb: &KnowledgeBase,
        partial_results: &[ExplorationResult],
    ) -> Result<LlmGeneratedReport> {
        // 准备输入数据摘要
        let facts_summary: String = kb.facts.iter()
            .take(30)
            .map(|f| format!("- [{}] {} ({})",
                format!("{:?}", f.confidence),
                f.claim.chars().take(200).collect::<String>(),
                f.source_url
            ))
            .collect::<Vec<_>>()
            .join("\n");

        let contradictions_summary: String = if kb.contradictions.is_empty() {
            "No contradictions identified.".to_string()
        } else {
            kb.contradictions.iter()
                .map(|c| format!("- {} vs {}", c.existing_claim, c.new_claim))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            r#"You are a Research Synthesis Expert. Analyze the collected research data and generate a comprehensive report.

## Original Query
{}

## Research Statistics
- Facts collected: {}
- Sources analyzed: {}
- Exploration angles: {}
- Recursion layers: {}

## Key Facts Extracted
{}

## Contradictions Identified
{}

## Your Task
Generate a comprehensive research report with the following sections:

1. **Executive Summary**: 3-5 paragraphs summarizing findings and answering the original query
2. **Key Findings**: 5-8 major findings with titles, detailed content, confidence levels, and source citations
3. **Detailed Analysis**: Organized by topic/section with in-depth discussion
4. **Contradictions**: Analysis of any conflicting information with assessment
5. **Information Gaps**: What important questions remain unanswered
6. **Methodology**: Brief description of the research approach

## Output Format (STRICT JSON)
{{
  "executive_summary": "Full executive summary text...",
  "key_findings": [
    {{
      "title": "Finding Title",
      "content": "Detailed explanation with evidence...",
      "confidence": "high",
      "citations": ["source_url_1", "source_url_2"]
    }}
  ],
  "detailed_analysis": "Full markdown analysis...",
  "contradictions": [
    {{
      "contradiction": {{"existing_claim": "...", "new_claim": "..."}},
      "analysis": "Explanation of the conflict...",
      "recommendation": "How to resolve or document"
    }}
  ],
  "gaps": [
    {{
      "gap": {{"description": "...", "importance": "high", "suggested_queries": []}},
      "impact": "Why this matters..."
    }}
  ],
  "methodology": "Description of research methodology..."
}}"#,
            query,
            kb.facts.len(),
            kb.sources.len(),
            partial_results.len(),
            kb.current_layer,
            facts_summary,
            contradictions_summary
        );

        // 调用 LLM
        let messages = vec![
            octos_core::Message::system("You are an expert research analyst. Generate comprehensive, objective reports with proper citations. Output valid JSON only."),
            octos_core::Message::user(prompt),
        ];

        let config = octos_llm::ChatConfig::default();
        let response = self.llm.chat(&messages, &[], &config).await
            .wrap_err("Failed to generate report with LLM")?;

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

        let report: LlmGeneratedReport = serde_json::from_str(json_str)
            .unwrap_or_else(|e| {
                warn!("Failed to parse LLM report JSON: {}. Using fallback.", e);
                self.generate_fallback_report(query, kb)
            });

        Ok(report)
    }

    /// 生成备用报告（当 LLM 解析失败时）
    fn generate_fallback_report(&self, query: &str, kb: &KnowledgeBase) -> LlmGeneratedReport {
        let key_findings: Vec<KeyFinding> = kb.facts.iter()
            .take(5)
            .enumerate()
            .map(|(i, f)| KeyFinding {
                title: format!("Finding {}", i + 1),
                content: f.claim.clone(),
                confidence: f.confidence.clone(),
                citations: vec![f.source_url.clone()],
            })
            .collect();

        LlmGeneratedReport {
            executive_summary: format!("Research on '{}' collected {} facts from {} sources.",
                query, kb.facts.len(), kb.sources.len()),
            key_findings,
            detailed_analysis: "See key findings above.".to_string(),
            contradictions: vec![],
            gaps: vec![],
            methodology: "Automated research with LLM synthesis (fallback mode).".to_string(),
        }
    }

    /// 提取核心发现
    async fn extract_key_findings(&self, kb: &KnowledgeBase) -> Result<Vec<KeyFinding>> {
        let mut findings = Vec::new();

        // 按置信度排序事实
        let mut sorted_facts: Vec<_> = kb.facts.iter().collect();
        sorted_facts.sort_by(|a, b| {
            let score_a = confidence_score(&a.confidence);
            let score_b = confidence_score(&b.confidence);
            score_b.cmp(&score_a)
        });

        // 选择前 5-8 个作为核心发现
        for (i, fact) in sorted_facts.iter().take(8).enumerate() {
            findings.push(KeyFinding {
                title: format!("Finding {}", i + 1),
                content: fact.claim.clone(),
                confidence: fact.confidence.clone(),
                citations: vec![fact.source_url.clone()],
            });
        }

        Ok(findings)
    }

    /// 分析矛盾
    async fn analyze_contradictions(
        &self,
        kb: &KnowledgeBase,
    ) -> Result<Vec<ContradictionSection>> {
        let mut sections = Vec::new();

        for contradiction in &kb.contradictions {
            sections.push(ContradictionSection {
                contradiction: contradiction.clone(),
                analysis: "Conflicting information identified.".to_string(),
                recommendation: if contradiction.resolution_needed {
                    "Further verification required.".to_string()
                } else {
                    "Document both perspectives.".to_string()
                },
            });
        }

        Ok(sections)
    }

    /// 识别信息缺口
    async fn identify_gaps(&self, kb: &KnowledgeBase) -> Result<Vec<GapSection>> {
        let mut sections = Vec::new();

        // 检查是否达到最小事实数
        if kb.facts.len() < self.config.quality_targets.min_facts {
            sections.push(GapSection {
                gap: Gap {
                    description: "Insufficient factual coverage".to_string(),
                    importance: Priority::High,
                    suggested_queries: vec!["Expand search terms".to_string()],
                },
                impact: "Research may be incomplete.".to_string(),
            });
        }

        // 检查覆盖度
        if kb.coverage_score < self.config.quality_targets.coverage_threshold {
            sections.push(GapSection {
                gap: Gap {
                    description: format!(
                        "Coverage below target: {:.0}%",
                        kb.coverage_score * 100.0
                    ),
                    importance: Priority::Medium,
                    suggested_queries: vec!["Broaden search angles".to_string()],
                },
                impact: "Some aspects may not be fully explored.".to_string(),
            });
        }

        Ok(sections)
    }

    /// 编译来源列表
    async fn compile_sources(&self, kb: &KnowledgeBase) -> Result<Vec<SourceCitation>> {
        let mut citations = Vec::new();

        for (i, (url, source)) in kb.sources.iter().enumerate() {
            citations.push(SourceCitation {
                id: format!("[{}]", i + 1),
                url: url.clone(),
                title: source.title.clone(),
                domain: source.domain.clone(),
                authority: format!("{:.0}%", source.authority_score * 100.0),
            });
        }

        Ok(citations)
    }

    /// 生成执行摘要
    async fn generate_executive_summary(
        &self,
        query: &str,
        kb: &KnowledgeBase,
        findings: &[KeyFinding],
    ) -> Result<String> {
        let mut summary = String::new();

        summary.push_str(&format!(
            "This report presents findings on '{}'. ",
            query
        ));
        summary.push_str(&format!(
            "The research analyzed {} sources and identified {} key facts. ",
            kb.sources.len(),
            kb.facts.len()
        ));

        if !findings.is_empty() {
            summary.push_str("The primary findings include: ");
            for (i, finding) in findings.iter().take(3).enumerate() {
                if i > 0 {
                    summary.push_str("; ");
                }
                summary.push_str(&finding.content);
            }
            summary.push_str(". ");
        }

        if kb.coverage_score >= self.config.quality_targets.coverage_threshold {
            summary.push_str("The research achieved comprehensive coverage of the topic. ");
        } else {
            summary.push_str("Some aspects may require additional investigation. ");
        }

        Ok(summary)
    }

    /// 生成详细分析
    async fn generate_detailed_analysis(&self, kb: &KnowledgeBase) -> Result<String> {
        let mut analysis = String::new();

        // 按类别分组事实
        let categories: std::collections::HashMap<_, Vec<_>> =
            kb.facts.iter().fold(std::collections::HashMap::new(), |mut acc, fact| {
                acc.entry(fact.category.clone())
                    .or_default()
                    .push(fact);
                acc
            });

        for (category, facts) in categories {
            analysis.push_str(&format!("\n### {:?}\n\n", category));
            for fact in facts.iter().take(5) {
                analysis.push_str(&format!("- {}\n", fact.claim));
            }
        }

        Ok(analysis)
    }

    /// 生成方法论说明
    async fn generate_methodology(
        &self,
        kb: &KnowledgeBase,
        partials: &[ExplorationResult],
    ) -> Result<String> {
        let mut methodology = String::new();

        methodology.push_str(&format!(
            "This research was conducted using automated deep research tools. "
        ));
        methodology.push_str(&format!(
            "The process involved {} parallel exploration angles across {} recursive layers. ",
            partials.len(),
            kb.current_layer
        ));
        methodology.push_str(&format!(
            "Sources were evaluated for authority and relevance, "
        ));
        methodology.push_str(&format!(
            "with a minimum threshold of {} facts and {:.0}% coverage. ",
            self.config.quality_targets.min_facts,
            self.config.quality_targets.coverage_threshold * 100.0
        ));

        Ok(methodology)
    }

    /// 估算 token 消耗
    fn estimate_tokens(&self, kb: &KnowledgeBase) -> u64 {
        let fact_chars: usize = kb.facts.iter().map(|f| f.claim.len()).sum();
        let source_chars: usize = kb.sources.values().map(|s| s.title.len()).sum();

        ((fact_chars + source_chars) as u64) / 4 // 粗略估算
    }

    /// 保存报告
    async fn save_report(&self, report: &ResearchReport) -> Result<()> {
        let filepath = self.research_dir.join("report.md");

        let content = self.format_report_markdown(report);

        tokio::fs::write(&filepath, content)
            .await
            .wrap_err_with(|| format!("Failed to write report to {}", filepath.display()))?;

        // 同时保存 JSON 版本
        let json_path = self.research_dir.join("report.json");
        let json = serde_json::to_string_pretty(report)
            .wrap_err("Failed to serialize report")?;
        tokio::fs::write(&json_path, json).await?;

        Ok(())
    }

    /// 格式化报告为 Markdown
    fn format_report_markdown(&self, report: &ResearchReport) -> String {
        let mut md = String::new();

        // 标题
        md.push_str(&format!("# Research Report: {}\n\n", report.query));

        // 元数据
        md.push_str(&format!(
            "*Generated: {} | Sources: {} | Facts: {}*\n\n",
            report.metadata.completed_at,
            report.metadata.sources_count,
            report.metadata.facts_count
        ));

        // 执行摘要
        md.push_str("## Executive Summary\n\n");
        md.push_str(&report.executive_summary);
        md.push_str("\n\n");

        // 核心发现
        md.push_str("## Key Findings\n\n");
        for (i, finding) in report.key_findings.iter().enumerate() {
            md.push_str(&format!(
                "### {}. {}\n\n",
                i + 1,
                finding.title
            ));
            md.push_str(&format!("**Confidence**: {:?}\n\n", finding.confidence));
            md.push_str(&format!("{}\n\n", finding.content));
            if !finding.citations.is_empty() {
                md.push_str(&format!("**Sources**: {}\n\n", finding.citations.join(", ")));
            }
        }

        // 详细分析
        md.push_str("## Detailed Analysis\n\n");
        md.push_str(&report.detailed_analysis);
        md.push_str("\n\n");

        // 矛盾
        if !report.contradictions.is_empty() {
            md.push_str("## Contradictions and Uncertainties\n\n");
            for section in &report.contradictions {
                md.push_str(&format!(
                    "- **{}** vs **{}**: {}\n",
                    section.contradiction.existing_claim,
                    section.contradiction.new_claim,
                    section.analysis
                ));
            }
            md.push_str("\n");
        }

        // 信息缺口
        if !report.gaps.is_empty() {
            md.push_str("## Information Gaps\n\n");
            for section in &report.gaps {
                md.push_str(&format!(
                    "- **{}**: {}\n",
                    section.gap.description, section.impact
                ));
            }
            md.push_str("\n");
        }

        // 方法论
        md.push_str("## Methodology\n\n");
        md.push_str(&report.methodology);
        md.push_str("\n\n");

        // 来源列表
        md.push_str("## Sources\n\n");
        for source in &report.sources {
            md.push_str(&format!(
                "{} **{}** ({}) - {}\n\n",
                source.id, source.title, source.authority, source.url
            ));
        }

        md
    }
}

/// 置信度转分数
fn confidence_score(confidence: &Confidence) -> u8 {
    match confidence {
        Confidence::High => 3,
        Confidence::Medium => 2,
        Confidence::Low => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use octos_core::Message;
    use octos_llm::{ChatConfig, ChatResponse, LlmProvider, ToolSpec};

    /// 测试用的 Dummy LLM Provider
    struct DummyLlmProvider;

    #[async_trait]
    impl LlmProvider for DummyLlmProvider {
        async fn chat(
            &self,
            _messages: &[Message],
            _tools: &[ToolSpec],
            _config: &ChatConfig,
        ) -> eyre::Result<ChatResponse> {
            Ok(ChatResponse {
                content: Some("{}".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                stop_reason: octos_llm::StopReason::EndTurn,
                usage: octos_llm::TokenUsage::default(),
            })
        }

        fn model_id(&self) -> &str {
            "dummy-model"
        }

        fn provider_name(&self) -> &str {
            "dummy"
        }
    }

    #[test]
    fn test_confidence_score() {
        assert_eq!(confidence_score(&Confidence::High), 3);
        assert_eq!(confidence_score(&Confidence::Medium), 2);
        assert_eq!(confidence_score(&Confidence::Low), 1);
    }

    #[test]
    fn test_format_report_markdown_structure() {
        let report = ResearchReport {
            query: "Test Query".to_string(),
            executive_summary: "This is a test summary.".to_string(),
            key_findings: vec![
                KeyFinding {
                    title: "Finding 1".to_string(),
                    content: "Test content".to_string(),
                    confidence: Confidence::High,
                    citations: vec!["https://test.com".to_string()],
                },
            ],
            detailed_analysis: "Detailed test analysis.".to_string(),
            contradictions: vec![],
            gaps: vec![],
            methodology: "Test methodology.".to_string(),
            sources: vec![
                SourceCitation {
                    id: "[1]".to_string(),
                    url: "https://test.com".to_string(),
                    title: "Test Source".to_string(),
                    domain: "test.com".to_string(),
                    authority: "80%".to_string(),
                },
            ],
            metadata: ReportMetadata {
                started_at: "2026-03-18T00:00:00Z".to_string(),
                completed_at: "2026-03-18T01:00:00Z".to_string(),
                angles_count: 5,
                facts_count: 25,
                sources_count: 10,
                recursion_depth: 2,
                estimated_tokens: 5000,
            },
        };

        // 创建临时 synthesizer 来测试格式化（测试不需要 LLM）
        let config = ResearchConfig::default();
        let synthesizer = Synthesizer::new(
            std::path::PathBuf::from("/tmp"),
            config,
            // 测试中使用 dummy LLM，但 format_report_markdown 不依赖 LLM
            Arc::new(DummyLlmProvider),
        );

        let md = synthesizer.format_report_markdown(&report);

        // 验证 Markdown 包含关键部分
        assert!(md.contains("# Research Report: Test Query"));
        assert!(md.contains("## Executive Summary"));
        assert!(md.contains("## Key Findings"));
        assert!(md.contains("## Detailed Analysis"));
        assert!(md.contains("## Methodology"));
        assert!(md.contains("## Sources"));
        assert!(md.contains("Test Source"));
        assert!(md.contains("https://test.com"));
    }

    #[test]
    fn test_estimate_tokens() {
        use std::collections::HashMap;

        let config = ResearchConfig::default();
        let synthesizer = Synthesizer::new(
            std::path::PathBuf::from("/tmp"),
            config,
            Arc::new(DummyLlmProvider),
        );

        let kb = KnowledgeBase {
            facts: vec![
                Fact {
                    id: "f1".to_string(),
                    claim: "a".repeat(400), // 400 字符
                    quote: "".to_string(),
                    confidence: Confidence::High,
                    category: FactCategory::Fact,
                    needs_verification: false,
                    verification_queries: vec![],
                    source_url: "".to_string(),
                    extracted_at: "".to_string(),
                },
            ],
            sources: {
                let mut s: HashMap<String, Source> = HashMap::new();
                s.insert("test".to_string(), Source {
                    url: "test".to_string(),
                    domain: "test.com".to_string(),
                    title: "b".repeat(400), // 400 字符
                    date_published: None,
                    date_modified: None,
                    author: None,
                    authority_score: 0.5,
                    relevance_score: 0.5,
                });
                s
            },
            ..Default::default()
        };

        let tokens = synthesizer.estimate_tokens(&kb);
        // (400 + 400) / 4 = 200
        assert_eq!(tokens, 200);
    }
}
