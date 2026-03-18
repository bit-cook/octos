//! Deep Research V2 - Agent-based 深度研究工具
//!
//! 融合 DEEP_RESEARCH_DESIGN.md 和 mofa-research-2.0 架构
//!
//! 架构流程：
//! 1. Entry Agent (Planning): 分析查询 → 生成研究计划
//! 2. Explorer Agents (Collection): 并行执行多角度搜索
//! 3. Recursion Controller: 管理 Layer 1-2-3 递归
//! 4. Synthesizer: 合成最终报告

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use eyre::{Result, WrapErr};
use serde_json::json;
use tracing::{info, warn};

use crate::tools::research::collector::CollectorAgent;
use crate::tools::research::recursion_controller::{RecursionController, RecursionDecision};
use crate::tools::research::synthesizer::Synthesizer;
use crate::tools::research::types::*;
use crate::tools::{Tool, ToolResult};
use crate::source_registry::SourceRegistry;

/// Deep Research V2 工具
pub struct DeepResearchV2Tool {
    working_dir: PathBuf,
    llm: Arc<dyn octos_llm::LlmProvider>,
}

impl DeepResearchV2Tool {
    pub fn new(
        working_dir: impl Into<PathBuf>,
        llm: Arc<dyn octos_llm::LlmProvider>,
    ) -> Self {
        Self {
            working_dir: working_dir.into(),
            llm,
        }
    }

    /// 执行深度研究主流程
    async fn execute_research(
        &self,
        query: &str,
        _depth: ResearchDepth,
        config: ResearchConfig,
    ) -> Result<ResearchReport> {
        let start_time = std::time::Instant::now();
        info!(query = %query, "Starting Deep Research V2");

        // 报告进度
        self.report_progress(&format!("开始深度研究: {}", query)).await;

        // 1. 加载 Source Registry
        let registry_path = self.working_dir.join("data/source_registry.toml");
        let source_registry = if registry_path.exists() {
            SourceRegistry::load(&registry_path).await.unwrap_or_default()
        } else {
            self.report_progress("使用默认源配置").await;
            SourceRegistry::default()
        };

        // 2. Entry Agent: 生成研究计划
        self.report_progress("生成研究计划...").await;
        let plan = self.generate_research_plan(query, &source_registry).await?;
        info!(angles = plan.angles.len(), "Research plan generated");

        // 3. 初始化知识库和递归控制器
        let mut kb = KnowledgeBase {
            current_layer: 1,
            coverage_score: 0.0,
            processed_angles: 0,
            ..Default::default()
        };
        let mut recursion_controller = RecursionController::new(config.clone());

        // 创建研究目录
        let research_dir = self.create_research_dir(query).await?;

        // 4. 执行递归研究
        let mut partial_results: Vec<ExplorationResult> = Vec::new();
        let mut current_layer = 1u8;

        // Layer 1: 使用研究计划中的角度
        self.report_progress("执行 Layer 1 探索...").await;
        info!(layer = 1, "Executing research layer");

        let layer_results = self
            .execute_parallel_collection(&plan.angles, &config, &research_dir, &kb)
            .await?;

        for result in &layer_results {
            kb.merge(result.clone());
            partial_results.push(result.clone());
        }

        self.report_progress(&format!(
            "Layer 1 完成: 收集 {} 个事实, 来源 {} 个",
            kb.facts.len(),
            kb.sources.len()
        )).await;

        // Layer 2+: 递归探索
        loop {
            // 检查递归决策（每层只调用一次）
            match recursion_controller.should_continue(&kb) {
                RecursionDecision::Continue { next_layer, queries } => {
                    current_layer = next_layer;
                    kb.current_layer = current_layer;

                    self.report_progress(&format!("执行 Layer {} 探索...", current_layer)).await;
                    info!(layer = current_layer, "Executing research layer");

                    if queries.is_empty() {
                        warn!("No queries generated for layer {}, stopping", current_layer);
                        break;
                    }

                    let layer_results = self
                        .execute_parallel_collection(&queries, &config, &research_dir, &kb)
                        .await?;

                    for result in &layer_results {
                        kb.merge(result.clone());
                        partial_results.push(result.clone());
                    }

                    self.report_progress(&format!(
                        "Layer {} 完成: 收集 {} 个事实, 来源 {} 个",
                        current_layer,
                        kb.facts.len(),
                        kb.sources.len()
                    )).await;
                }
                RecursionDecision::Stop { reason } => {
                    info!(reason = reason.as_str(), "Stopping recursion");
                    break;
                }
            }
        }

        // 5. 合成报告
        self.report_progress("合成最终报告...").await;
        let synthesizer = Synthesizer::new(research_dir.clone(), config, self.llm.clone());
        let report = synthesizer.synthesize(query, &kb, &partial_results).await?;

        let duration = start_time.elapsed();
        info!(
            duration = ?duration,
            facts = kb.facts.len(),
            sources = kb.sources.len(),
            "Deep Research V2 completed"
        );

        self.report_progress(&format!(
            "研究完成! 耗时 {:?}, 事实: {}, 来源: {}",
            duration,
            kb.facts.len(),
            kb.sources.len()
        )).await;

        Ok(report)
    }

    /// 生成研究计划 (Entry Agent)
    async fn generate_research_plan(
        &self,
        query: &str,
        source_registry: &SourceRegistry,
    ) -> Result<ResearchPlan> {
        // 读取 planner prompt
        let prompt_path = self.working_dir.join("src/prompts/research_planner.txt");
        let prompt_template = if prompt_path.exists() {
            tokio::fs::read_to_string(&prompt_path).await.unwrap_or_else(|_| {
                Self::default_planner_prompt().to_string()
            })
        } else {
            Self::default_planner_prompt().to_string()
        };

        // 构建输入
        let prompt = prompt_template
            .replace("{{QUERY}}", query)
            .replace("{{SOURCE_REGISTRY}}", &source_registry.to_prompt_context());

        // 调用 LLM 生成计划
        let messages = vec![
            octos_core::Message::system("You are a research planning expert."),
            octos_core::Message::user(prompt),
        ];

        let config = octos_llm::ChatConfig::default();
        let response = self.llm.chat(&messages, &[], &config).await
            .wrap_err("Failed to generate research plan")?;

        // 解析 JSON 响应
        let content = response.content.as_deref().unwrap_or("").trim();
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

        let plan: ResearchPlan = serde_json::from_str(json_str)
            .wrap_err_with(|| format!("Failed to parse research plan: {}", content))?
        ;

        Ok(plan)
    }

    /// 并行执行多角度收集
    async fn execute_parallel_collection(
        &self,
        angles: &[ResearchAngle],
        config: &ResearchConfig,
        research_dir: &PathBuf,
        parent_kb: &KnowledgeBase,
    ) -> Result<Vec<ExplorationResult>> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent_angles));
        let mut handles = Vec::new();

        for angle in angles {
            let permit = semaphore.clone().acquire_owned().await?;
            let angle = angle.clone();
            let config = config.clone();
            let research_dir = research_dir.clone();
            let llm = self.llm.clone();
            let parent_kb = parent_kb.clone();

            let handle = tokio::spawn(async move {
                let _permit = permit; // 保持 permit 存活

                let collector = CollectorAgent::new(
                    angle,
                    config,
                    research_dir,
                    llm,
                );

                match collector.collect(&parent_kb).await {
                    Ok(result) => Some(result),
                    Err(e) => {
                        warn!("Collector failed: {}", e);
                        None
                    }
                }
            });

            handles.push(handle);
        }

        // 收集结果
        let mut results = Vec::new();
        for handle in handles {
            if let Ok(Some(result)) = handle.await {
                results.push(result);
            }
        }

        Ok(results)
    }

    /// 创建研究目录
    async fn create_research_dir(&self, query: &str) -> Result<PathBuf> {
        // 生成目录名
        let slug: String = query
            .to_lowercase()
            .replace(' ', "_")
            .replace(|c: char| !c.is_alphanumeric() && c != '_', "")
            .trim_end_matches('_')
            .to_string();

        let dir_name = format!("{}_{}",
            &slug[..slug.len().min(30)],
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        );

        let research_dir = self.working_dir.join("research_output").join(&dir_name);
        tokio::fs::create_dir_all(&research_dir).await?;

        Ok(research_dir)
    }

    /// 报告进度
    async fn report_progress(&self, message: &str) {
        // 通过 ToolContext 报告进度
        let _ = crate::tools::TOOL_CTX.try_with(|ctx| {
            ctx.reporter.report(crate::progress::ProgressEvent::ToolProgress {
                name: "deep_research_v2".to_string(),
                tool_id: ctx.tool_id.clone(),
                message: message.to_string(),
            });
        });
    }

    /// 默认 planner prompt
    fn default_planner_prompt() -> &'static str {
        r#"You are a research planning expert. Analyze the query and generate a structured research plan.

Query: {{QUERY}}

Generate a JSON research plan with:
- intent: "factual" | "comparative" | "trend" | "overview"
- core_topics: 1-3 main topics
- angles: 5-15 research angles covering different dimensions
- recommended_depth: "quick" | "standard" | "deep" | "thorough"
- recursion_plan: layer descriptions for deep research
- quality_targets: min_facts, target_facts, coverage_threshold

Output JSON format:
{
  "intent": "factual",
  "core_topics": ["topic1", "topic2"],
  "angles": [
    {
      "task": "search query text",
      "label": "descriptive label",
      "dimension": "technical|business|political|social|historical|market",
      "language": "en|zh|es|ja|ko|de|fr",
      "engines": ["serper", "baidu"],
      "portals": []
    }
  ],
  "recommended_depth": "deep",
  "recursion_plan": {
    "layer1_description": "What is happening?",
    "layer2_description": "Background and context",
    "layer3_description": "Impact and reactions"
  },
  "quality_targets": {
    "min_facts": 25,
    "target_facts": 40,
    "coverage_threshold": 0.85,
    "min_sources": 15,
    "min_source_types": 3
  },
  "languages": ["en", "zh"]
}"#
    }
}

#[async_trait]
impl Tool for DeepResearchV2Tool {
    fn name(&self) -> &str {
        "deep_research_v2"
    }

    fn description(&self) -> &str {
        r#"Execute deep, multi-layer research using Agent-based architecture.

This tool performs comprehensive research through:
1. Planning: Analyze query and create research angles
2. Collection: Parallel exploration from multiple sources
3. Recursion: Layer 1-2-3 depth exploration (What → Background → Impact)
4. Synthesis: Generate comprehensive report with citations

Use for complex queries requiring thorough investigation across multiple sources.

Parameters:
- query: The research question or topic
- depth: "quick", "standard", "deep", or "thorough" (default: deep)
- max_recursion_depth: 1-5 layers (default: 3)
"#
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The research question or topic to investigate"
                },
                "depth": {
                    "type": "string",
                    "enum": ["quick", "standard", "deep", "thorough"],
                    "default": "deep",
                    "description": "Research depth level"
                },
                "max_recursion_depth": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 5,
                    "default": 3,
                    "description": "Maximum recursion layers for deep exploration"
                },
                "min_facts": {
                    "type": "integer",
                    "minimum": 5,
                    "default": 25,
                    "description": "Minimum facts to collect"
                },
                "coverage_threshold": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0,
                    "default": 0.85,
                    "description": "Coverage quality threshold"
                }
            },
            "required": ["query"]
        })
    }

    fn tags(&self) -> &[&str] {
        &["research", "web"]
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        // 解析参数
        let query = args["query"]
            .as_str()
            .ok_or_else(|| eyre::eyre!("Missing 'query' parameter"))?
        ;

        let depth = match args.get("depth").and_then(|d| d.as_str()) {
            Some("quick") => ResearchDepth::Quick,
            Some("standard") => ResearchDepth::Standard,
            Some("thorough") => ResearchDepth::Thorough,
            _ => ResearchDepth::Deep,
        };

        let max_recursion = args["max_recursion_depth"].as_u64().unwrap_or(3) as u8;
        let min_facts = args["min_facts"].as_u64().unwrap_or(25) as usize;
        let coverage = args["coverage_threshold"].as_f64().unwrap_or(0.85);

        // 构建配置
        let config = ResearchConfig {
            default_depth: depth,
            max_recursion_depth: max_recursion,
            quality_targets: QualityTargets {
                min_facts,
                coverage_threshold: coverage,
                ..Default::default()
            },
            ..Default::default()
        };

        // 执行研究
        match self.execute_research(query, depth, config).await {
            Ok(report) => {
                // 格式化输出
                let output = format!(
                    "# Research Report: {}\n\n{}\n\n## Key Findings\n\n{}\n\n## Methodology\n\n{}\n\nReport saved to: research_output/",
                    report.query,
                    report.executive_summary,
                    report.key_findings
                        .iter()
                        .enumerate()
                        .map(|(i, f)| format!("{}. {} ({} confidence)\n{}",
                            i + 1,
                            f.title,
                            format!("{:?}", f.confidence).to_lowercase(),
                            f.content
                        ))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                    report.methodology
                );

                Ok(ToolResult {
                    output,
                    success: true,
                    ..Default::default()
                })
            }
            Err(e) => Ok(ToolResult {
                output: format!("Research failed: {}", e),
                success: false,
                ..Default::default()
            }),
        }
    }
}
