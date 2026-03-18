//! Deep Research V2 - Core Types
//!
//! 融合 DEEP_RESEARCH_DESIGN.md 和 mofa-research-2.0 的类型定义

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// -----------------------------------------------------------------------------
/// Phase 1: Planning - 研究计划
/// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResearchPlan {
    /// 研究意图类型
    pub intent: ResearchIntent,

    /// 核心主题（1-3个）
    pub core_topics: Vec<String>,

    /// 搜索角度（5-20个）
    pub angles: Vec<ResearchAngle>,

    /// 推荐深度
    pub recommended_depth: ResearchDepth,

    /// 递归规划（mofa-research-2.0 特性）
    pub recursion_plan: RecursionPlan,

    /// 质量目标（mofa-research-2.0 特性）
    pub quality_targets: QualityTargets,

    /// 多语言配置
    pub languages: Vec<Language>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchIntent {
    /// 事实查询
    Factual,
    /// 对比分析
    Comparative,
    /// 趋势预测
    Trend,
    /// 全面概述
    Overview,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchDepth {
    /// 快速：1-2个角度
    Quick,
    /// 标准：3-5个角度
    Standard,
    /// 深入：6-10个角度
    Deep,
    /// 全面：10+个角度
    Thorough,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResearchAngle {
    /// 搜索任务（查询字符串）
    pub task: String,

    /// 标签描述
    pub label: String,

    /// 维度分类
    pub dimension: Dimension,

    /// 语言
    pub language: Language,

    /// 分配的搜索引擎
    pub engines: Vec<String>,

    /// 分配的门户/数据源
    pub portals: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    /// 技术细节
    Technical,
    /// 商业影响
    Business,
    /// 政策法规
    Political,
    /// 社会反应
    Social,
    /// 历史背景
    Historical,
    /// 市场数据
    Market,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Zh,
    Es,
    Ja,
    Ko,
    De,
    Fr,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Zh => "zh",
            Language::Es => "es",
            Language::Ja => "ja",
            Language::Ko => "ko",
            Language::De => "de",
            Language::Fr => "fr",
        }
    }
}

/// 递归规划（mofa-research-2.0 News Trail 模式）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecursionPlan {
    /// Layer 1: What happened? 事实层
    pub layer1_description: String,

    /// Layer 2: Background/Why? 背景层
    pub layer2_description: String,

    /// Layer 3: Impact/Reactions? 影响层
    pub layer3_description: String,
}

/// 质量目标（硬性指标）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QualityTargets {
    /// 最少事实数
    pub min_facts: usize,

    /// 目标事实数
    pub target_facts: usize,

    /// 覆盖度阈值（0-1）
    pub coverage_threshold: f64,

    /// 最少来源数
    pub min_sources: usize,

    /// 最少来源类型数
    pub min_source_types: usize,
}

impl Default for QualityTargets {
    fn default() -> Self {
        Self {
            min_facts: 25,
            target_facts: 40,
            coverage_threshold: 0.85,
            min_sources: 15,
            min_source_types: 3,
        }
    }
}

/// -----------------------------------------------------------------------------
/// ------------------------------------------------------------------------------
/// Phase 2: Collection - 探索结果
/// ------------------------------------------------------------------------------

/// 单页提取结果（LLM 返回的临时结构）
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PageExtraction {
    pub facts: Vec<Fact>,
    pub sources: Vec<Source>,
    pub recursion_candidates: Vec<RecursionCandidate>,
    pub contradictions: Vec<Contradiction>,
    pub authority_score: f64,
    pub relevance_score: f64,
}

/// 单个角度的探索结果（由 Collector Sub-Agent 返回）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExplorationResult {
    /// 对应的角度
    pub angle: ResearchAngle,

    /// 提取的事实
    pub facts: Vec<Fact>,

    /// 来源评估
    pub sources: Vec<Source>,

    /// 递归候选（关键！用于触发下一层）
    pub recursion_candidates: Vec<RecursionCandidate>,

    /// 发现的矛盾
    pub contradictions: Vec<Contradiction>,

    /// 权威性评估
    pub authority_assessment: AuthorityAssessment,

    /// 后续链接
    pub follow_ups: FollowUpLinks,

    /// 覆盖度贡献（0-1）
    pub coverage_contribution: f64,

    /// 子问题（用于递归）
    pub sub_questions: Vec<String>,
}

/// 事实条目
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Fact {
    /// 唯一ID
    pub id: String,

    /// 主张/声明
    pub claim: String,

    /// 原文引用
    pub quote: String,

    /// 置信度
    pub confidence: Confidence,

    /// 分类
    pub category: FactCategory,

    /// 是否需要验证
    pub needs_verification: bool,

    /// 验证查询建议
    pub verification_queries: Vec<String>,

    /// 来源URL
    pub source_url: String,

    /// 提取时间
    pub extracted_at: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,    // 来源不明，推测，矛盾
    Medium, // 次要来源，合理，有支持证据
    High,   // 主要来源，清晰证据，权威
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FactCategory {
    Fact,       // 客观事实
    Opinion,    // 观点
    Prediction, // 预测
    Data,       // 数据
}

/// 来源评估
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Source {
    /// URL
    pub url: String,

    /// 域名
    pub domain: String,

    /// 标题
    pub title: String,

    /// 发布日期
    pub date_published: Option<String>,

    /// 修改日期
    pub date_modified: Option<String>,

    /// 作者
    pub author: Option<String>,

    /// 权威性评分（0-1）
    pub authority_score: f64,

    /// 相关性评分（0-1）
    pub relevance_score: f64,
}

/// 递归候选（触发下一层探索的关键）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecursionCandidate {
    /// 实体类型
    pub entity_type: EntityType,

    /// 实体名称
    pub entity: String,

    /// 上下文描述
    pub context: String,

    /// 优先级
    pub priority: Priority,

    /// Layer 2 查询建议（背景）
    pub layer2_queries: Vec<String>,

    /// Layer 3 查询建议（影响）
    pub layer3_queries: Vec<String>,

    /// 原因说明
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Event,      // 事件
    Person,     // 人物
    Company,    // 公司
    Data,       // 数据/统计
    Policy,     // 政策/法规
    Technology, // 技术
    Reaction,   // 反应/声明
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    Low,
}

/// 矛盾发现
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Contradiction {
    /// 已有主张
    pub existing_claim: String,

    /// 新主张
    pub new_claim: String,

    /// 矛盾解释
    pub explanation: String,

    /// 是否需要解决
    pub resolution_needed: bool,

    /// 涉及的事实ID
    pub fact_ids: Vec<String>,
}

/// 权威性评估
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthorityAssessment {
    /// 总体评分（0-1）
    pub score: f64,

    /// 评估理由
    pub reasoning: String,

    /// 红旗警告
    pub red_flags: Vec<String>,
}

/// 后续链接
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FollowUpLinks {
    /// 内部链接（同域名）
    pub internal: Vec<Link>,

    /// 外部引用
    pub external: Vec<Link>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Link {
    /// 链接文本
    pub text: String,

    /// URL
    pub url: String,

    /// 优先级
    pub priority: Priority,

    /// 探索原因
    pub reason: String,
}

/// -----------------------------------------------------------------------------
/// Phase 3: Knowledge Base - 知识聚合
/// -----------------------------------------------------------------------------

/// 知识库（聚合所有探索结果）
#[derive(Debug, Default, Clone)]
pub struct KnowledgeBase {
    /// 所有事实
    pub facts: Vec<Fact>,

    /// 来源映射（URL -> Source）
    pub sources: HashMap<String, Source>,

    /// 矛盾列表
    pub contradictions: Vec<Contradiction>,

    /// 递归候选池
    pub recursion_candidates: Vec<RecursionCandidate>,

    /// 信息缺口
    pub gaps: Vec<Gap>,

    /// 当前覆盖度（0-1）
    pub coverage_score: f64,

    /// 已处理的角度数
    pub processed_angles: usize,

    /// 当前递归层
    pub current_layer: u8,
}

/// 信息缺口
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Gap {
    /// 缺口描述
    pub description: String,

    /// 重要性
    pub importance: Priority,

    /// 建议查询（用于填补）
    pub suggested_queries: Vec<String>,
}

impl KnowledgeBase {
    /// 合并新的探索结果
    pub fn merge(&mut self, result: ExplorationResult) {
        // 添加事实
        for fact in result.facts {
            self.facts.push(fact);
        }

        // 添加来源
        for source in result.sources {
            self.sources.insert(source.url.clone(), source);
        }

        // 添加矛盾
        for contradiction in result.contradictions {
            self.contradictions.push(contradiction);
        }

        // 添加递归候选
        for candidate in result.recursion_candidates {
            self.recursion_candidates.push(candidate);
        }

        // 更新覆盖度（简单累加，实际应该用更复杂的算法）
        self.coverage_score += result.coverage_contribution;
        if self.coverage_score > 1.0 {
            self.coverage_score = 1.0;
        }

        self.processed_angles += 1;
    }

    /// 获取高优先级递归候选
    pub fn high_priority_candidates(&self) -> Vec<&RecursionCandidate> {
        self.recursion_candidates
            .iter()
            .filter(|c| c.priority == Priority::High)
            .collect()
    }

    /// 统计不同来源类型数
    pub fn source_type_count(&self) -> usize {
        // 简化实现：按域名后缀分类
        let domains: std::collections::HashSet<_> = self
            .sources
            .values()
            .map(|s| s.domain.clone())
            .collect();
        domains.len()
    }
}

/// -----------------------------------------------------------------------------
/// Phase 4: Synthesis - 合成报告
/// -----------------------------------------------------------------------------

/// 研究报告
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResearchReport {
    /// 查询
    pub query: String,

    /// 执行摘要
    pub executive_summary: String,

    /// 核心发现（5-8条）
    pub key_findings: Vec<KeyFinding>,

    /// 详细分析
    pub detailed_analysis: String,

    /// 矛盾与不确定性
    pub contradictions: Vec<ContradictionSection>,

    /// 信息缺口
    pub gaps: Vec<GapSection>,

    /// 方法论说明
    pub methodology: String,

    /// 来源列表
    pub sources: Vec<SourceCitation>,

    /// 元数据
    pub metadata: ReportMetadata,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeyFinding {
    /// 标题
    pub title: String,

    /// 内容
    pub content: String,

    /// 置信度
    pub confidence: Confidence,

    /// 来源引用
    pub citations: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContradictionSection {
    /// 矛盾点
    pub contradiction: Contradiction,

    /// 分析
    pub analysis: String,

    /// 建议
    pub recommendation: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GapSection {
    /// 缺口
    pub gap: Gap,

    /// 影响
    pub impact: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceCitation {
    /// 编号
    pub id: String,

    /// URL
    pub url: String,

    /// 标题
    pub title: String,

    /// 域名
    pub domain: String,

    /// 权威性
    pub authority: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReportMetadata {
    /// 研究开始时间
    pub started_at: String,

    /// 研究结束时间
    pub completed_at: String,

    /// 搜索角度数
    pub angles_count: usize,

    /// 收集事实数
    pub facts_count: usize,

    /// 来源数
    pub sources_count: usize,

    /// 递归层数
    pub recursion_depth: u8,

    /// 总token消耗（估算）
    pub estimated_tokens: u64,
}

/// -----------------------------------------------------------------------------
/// Configuration - 研究配置
/// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResearchConfig {
    /// 默认深度
    pub default_depth: ResearchDepth,

    /// 最大并发角度数
    pub max_concurrent_angles: usize,

    /// 最大递归深度
    pub max_recursion_depth: u8,

    /// 最小递归深度（必须完成的层数）
    pub min_recursion_depth: u8,

    /// 质量目标
    pub quality_targets: QualityTargets,

    /// 每个角度的最大搜索结果数
    pub max_results_per_angle: usize,

    /// 每个角度的最大页面获取数
    pub max_pages_per_angle: usize,

    /// 子代理超时（秒）
    pub agent_timeout_secs: u64,

    /// 停滞限制（连续无新发现的最大次数）
    pub stagnation_limit: u8,

    /// 是否启用双语
    pub cross_language: bool,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            default_depth: ResearchDepth::Deep,
            max_concurrent_angles: 5,
            max_recursion_depth: 3,
            min_recursion_depth: 1,
            quality_targets: QualityTargets::default(),
            max_results_per_angle: 8,
            max_pages_per_angle: 5,
            agent_timeout_secs: 600,
            stagnation_limit: 5,
            cross_language: true,
        }
    }
}

/// -----------------------------------------------------------------------------
/// Search Result - 搜索结果（来自各搜索引擎）
/// -----------------------------------------------------------------------------

/// 通用搜索结果
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchResult {
    /// 查询
    pub query: String,

    /// 结果项
    pub items: Vec<SearchResultItem>,

    /// 搜索引擎
    pub engine: String,

    /// 语言
    pub language: Language,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchResultItem {
    /// 标题
    pub title: String,

    /// URL
    pub url: String,

    /// 摘要
    pub snippet: String,

    /// 发布时间
    pub published_date: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_knowledge_base_merge() {
        let mut kb = KnowledgeBase::default();

        let result = ExplorationResult {
            angle: ResearchAngle {
                task: "test".to_string(),
                label: "Test".to_string(),
                dimension: Dimension::Technical,
                language: Language::En,
                engines: vec!["serper".to_string()],
                portals: vec![],
            },
            facts: vec![
                Fact {
                    id: "f1".to_string(),
                    claim: "Test claim".to_string(),
                    quote: "Test quote".to_string(),
                    confidence: Confidence::High,
                    category: FactCategory::Fact,
                    needs_verification: false,
                    verification_queries: vec![],
                    source_url: "https://test.com".to_string(),
                    extracted_at: "2026-03-18".to_string(),
                },
            ],
            sources: vec![Source {
                url: "https://test.com".to_string(),
                domain: "test.com".to_string(),
                title: "Test Source".to_string(),
                date_published: None,
                date_modified: None,
                author: None,
                authority_score: 0.8,
                relevance_score: 0.9,
            }],
            recursion_candidates: vec![RecursionCandidate {
                entity_type: EntityType::Event,
                entity: "Test Event".to_string(),
                context: "Test context".to_string(),
                priority: Priority::High,
                layer2_queries: vec!["query2".to_string()],
                layer3_queries: vec!["query3".to_string()],
                reason: "Test reason".to_string(),
            }],
            contradictions: vec![],
            authority_assessment: AuthorityAssessment {
                score: 0.8,
                reasoning: "Good source".to_string(),
                red_flags: vec![],
            },
            follow_ups: FollowUpLinks {
                internal: vec![],
                external: vec![],
            },
            coverage_contribution: 0.15,
            sub_questions: vec![],
        };

        kb.merge(result);

        assert_eq!(kb.facts.len(), 1);
        assert_eq!(kb.sources.len(), 1);
        assert_eq!(kb.recursion_candidates.len(), 1);
        assert_eq!(kb.processed_angles, 1);
        assert!((kb.coverage_score - 0.15).abs() < 0.001);
    }

    #[test]
    fn test_knowledge_base_high_priority_candidates() {
        let mut kb = KnowledgeBase::default();
        kb.recursion_candidates = vec![
            RecursionCandidate {
                entity_type: EntityType::Event,
                entity: "High Priority".to_string(),
                context: "test".to_string(),
                priority: Priority::High,
                layer2_queries: vec![],
                layer3_queries: vec![],
                reason: "test".to_string(),
            },
            RecursionCandidate {
                entity_type: EntityType::Event,
                entity: "Low Priority".to_string(),
                context: "test".to_string(),
                priority: Priority::Low,
                layer2_queries: vec![],
                layer3_queries: vec![],
                reason: "test".to_string(),
            },
        ];

        let high_priority = kb.high_priority_candidates();
        assert_eq!(high_priority.len(), 1);
        assert_eq!(high_priority[0].entity, "High Priority");
    }

    #[test]
    fn test_quality_targets_default() {
        let qt = QualityTargets::default();
        assert_eq!(qt.min_facts, 25);
        assert_eq!(qt.target_facts, 40);
        assert!((qt.coverage_threshold - 0.85).abs() < 0.001);
        assert_eq!(qt.min_sources, 15);
        assert_eq!(qt.min_source_types, 3);
    }

    #[test]
    fn test_research_config_default() {
        let config = ResearchConfig::default();
        assert_eq!(config.max_concurrent_angles, 5);
        assert_eq!(config.max_recursion_depth, 3);
        assert_eq!(config.min_recursion_depth, 1);
        assert_eq!(config.max_results_per_angle, 8);
        assert_eq!(config.max_pages_per_angle, 5);
    }

    #[test]
    fn test_language_as_str() {
        assert_eq!(Language::En.as_str(), "en");
        assert_eq!(Language::Zh.as_str(), "zh");
        assert_eq!(Language::Ja.as_str(), "ja");
    }

    #[test]
    fn test_confidence_score_ordering() {
        // 确保 Confidence 可以比较
        assert!(Confidence::High > Confidence::Medium, "High should be greater than Medium");
        assert!(Confidence::Medium > Confidence::Low, "Medium should be greater than Low");
    }

    #[test]
    fn test_fact_category_hash() {
        // 测试 FactCategory 实现了 Hash，可以用作 HashMap key
        use std::collections::HashMap;
        let mut map: HashMap<FactCategory, String> = HashMap::new();
        map.insert(FactCategory::Fact, "fact".to_string());
        map.insert(FactCategory::Opinion, "opinion".to_string());
        map.insert(FactCategory::Data, "data".to_string());
        assert_eq!(map.len(), 3);
    }
}
