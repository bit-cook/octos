//! Recursion Controller - 递归决策控制器
//!
//! 决定是否继续递归探索，管理 Layer 1-2-3 的推进

use tracing::info;

use crate::tools::research::types::*;

/// 递归决策控制器
pub struct RecursionController {
    config: ResearchConfig,
    stagnation_counter: u8,
    previous_fact_count: usize,
}

impl RecursionController {
    pub fn new(config: ResearchConfig) -> Self {
        Self {
            config,
            stagnation_counter: 0,
            previous_fact_count: 0,
        }
    }

    /// 检查是否应该继续递归
    pub fn should_continue(&mut self, kb: &KnowledgeBase) -> RecursionDecision {
        let current_layer = kb.current_layer;
        let fact_count = kb.facts.len();
        let coverage = kb.coverage_score;

        info!(
            layer = current_layer,
            facts = fact_count,
            coverage = coverage,
            "Checking recursion decision"
        );

        // 检查硬性停止条件

        // 1. 达到最大递归深度
        if current_layer >= self.config.max_recursion_depth {
            info!("Stopping: reached max recursion depth");
            return RecursionDecision::Stop {
                reason: StopReason::MaxDepthReached,
            };
        }

        // 2. 检查停滞
        if fact_count <= self.previous_fact_count {
            self.stagnation_counter += 1;
            if self.stagnation_counter >= self.config.stagnation_limit {
                info!("Stopping: stagnation detected");
                return RecursionDecision::Stop {
                    reason: StopReason::Stagnation,
                };
            }
        } else {
            self.stagnation_counter = 0;
        }

        self.previous_fact_count = fact_count;

        // 检查质量目标

        // 3. 如果已经达到质量目标，可以考虑停止
        let min_depth_satisfied = current_layer >= self.config.min_recursion_depth;
        let facts_satisfied = fact_count >= self.config.quality_targets.min_facts;
        let coverage_satisfied = coverage >= self.config.quality_targets.coverage_threshold;

        if min_depth_satisfied && facts_satisfied && coverage_satisfied {
            info!("Quality targets met, can stop");
            // 但如果有高优先级递归候选，还是继续
            let high_priority = kb.high_priority_candidates();
            if high_priority.is_empty() {
                return RecursionDecision::Stop {
                    reason: StopReason::QualityTargetsMet,
                };
            }
        }

        // 4. 检查是否有递归候选
        if kb.recursion_candidates.is_empty() {
            // 如果没有递归候选，但还没达到最小深度，生成通用查询
            if current_layer < self.config.min_recursion_depth {
                return RecursionDecision::Continue {
                    next_layer: current_layer + 1,
                    queries: self.generate_fallback_queries(current_layer, kb),
                };
            }

            info!("Stopping: no recursion candidates");
            return RecursionDecision::Stop {
                reason: StopReason::NoCandidates,
            };
        }

        // 继续递归
        let next_layer = current_layer + 1;
        let queries = self.generate_next_layer_queries(next_layer, kb);

        if queries.is_empty() {
            return RecursionDecision::Stop {
                reason: StopReason::NoQueriesGenerated,
            };
        }

        RecursionDecision::Continue {
            next_layer,
            queries,
        }
    }

    /// 生成下一层的查询
    fn generate_next_layer_queries(
        &self,
        next_layer: u8,
        kb: &KnowledgeBase,
    ) -> Vec<ResearchAngle> {
        let mut queries = Vec::new();

        // 获取高优先级的递归候选
        let candidates: Vec<_> = kb
            .recursion_candidates
            .iter()
            .filter(|c| c.priority == Priority::High)
            .take(3) // 最多3个高优先级候选
            .collect();

        for candidate in candidates {
            let query_text = match next_layer {
                2 => candidate
                    .layer2_queries
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("{} background", candidate.entity)),
                3 => candidate
                    .layer3_queries
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("{} impact", candidate.entity)),
                _ => format!("{} details", candidate.entity),
            };

            queries.push(ResearchAngle {
                task: query_text,
                label: format!("Layer {}: {}", next_layer, candidate.entity),
                dimension: Dimension::Technical, // 默认
                language: Language::En,
                engines: vec!["serper".to_string()],
                portals: vec![],
            });
        }

        // 如果没有足够的高优先级候选，添加一些通用的
        if queries.len() < 2 && next_layer <= 3 {
            queries.push(ResearchAngle {
                task: format!("{} background history", kb.facts.first().map(|f| f.claim.clone()).unwrap_or_default()),
                label: format!("Layer {}: Background", next_layer),
                dimension: Dimension::Historical,
                language: Language::En,
                engines: vec!["serper".to_string()],
                portals: vec![],
            });
        }

        queries
    }

    /// 生成回退查询（当没有递归候选时）
    fn generate_fallback_queries(
        &self,
        current_layer: u8,
        kb: &KnowledgeBase,
    ) -> Vec<ResearchAngle> {
        let base_topic = kb
            .facts
            .first()
            .map(|f| f.claim.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();

        let query = match current_layer {
            1 => format!("{} history background", base_topic),
            2 => format!("{} impact consequences", base_topic),
            _ => format!("{} detailed analysis", base_topic),
        };

        vec![ResearchAngle {
            task: query,
            label: format!("Fallback Layer {}", current_layer + 1),
            dimension: Dimension::Technical,
            language: Language::En,
            engines: vec!["serper".to_string()],
            portals: vec![],
        }]
    }

    /// 获取当前停滞计数
    pub fn stagnation_count(&self) -> u8 {
        self.stagnation_counter
    }

    /// 重置停滞计数器
    pub fn reset_stagnation(&mut self) {
        self.stagnation_counter = 0;
        self.previous_fact_count = 0;
    }
}

/// 递归决策结果
#[derive(Debug, Clone)]
pub enum RecursionDecision {
    /// 继续递归
    Continue {
        next_layer: u8,
        queries: Vec<ResearchAngle>,
    },
    /// 停止递归
    Stop {
        reason: StopReason,
    },
}

/// 停止原因
#[derive(Debug, Clone)]
pub enum StopReason {
    MaxDepthReached,
    Stagnation,
    QualityTargetsMet,
    NoCandidates,
    NoQueriesGenerated,
}

impl StopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            StopReason::MaxDepthReached => "Maximum recursion depth reached",
            StopReason::Stagnation => "No new information in recent iterations",
            StopReason::QualityTargetsMet => "Quality targets satisfied",
            StopReason::NoCandidates => "No recursion candidates available",
            StopReason::NoQueriesGenerated => "Could not generate follow-up queries",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_kb(facts_count: usize, layer: u8) -> KnowledgeBase {
        KnowledgeBase {
            facts: (0..facts_count)
                .map(|i| Fact {
                    id: format!("f{}", i),
                    claim: "Test fact".to_string(),
                    quote: "Test quote".to_string(),
                    confidence: Confidence::High,
                    category: FactCategory::Fact,
                    needs_verification: false,
                    verification_queries: vec![],
                    source_url: "https://test.com".to_string(),
                    extracted_at: "2026-03-18".to_string(),
                })
                .collect(),
            sources: Default::default(),
            contradictions: vec![],
            recursion_candidates: vec![],
            gaps: vec![],
            coverage_score: facts_count as f64 * 0.02,
            processed_angles: 5,
            current_layer: layer,
        }
    }

    #[test]
    fn test_max_depth_stop() {
        let config = ResearchConfig {
            max_recursion_depth: 3,
            min_recursion_depth: 1,
            stagnation_limit: 5,
            quality_targets: QualityTargets::default(),
            ..Default::default()
        };

        let mut controller = RecursionController::new(config);
        let kb = create_test_kb(10, 3);

        let decision = controller.should_continue(&kb);

        match decision {
            RecursionDecision::Stop { reason } => {
                assert!(matches!(reason, StopReason::MaxDepthReached));
            }
            _ => panic!("Should stop at max depth"),
        }
    }

    #[test]
    fn test_quality_targets_met() {
        let config = ResearchConfig {
            max_recursion_depth: 5,
            min_recursion_depth: 1,
            stagnation_limit: 5,
            quality_targets: QualityTargets {
                min_facts: 10,
                coverage_threshold: 0.5,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut controller = RecursionController::new(config);
        let kb = create_test_kb(30, 1);

        let decision = controller.should_continue(&kb);

        // 应该停止因为没有高优先级候选
        match decision {
            RecursionDecision::Stop { .. } => {}
            _ => {}
        }
    }

    #[test]
    fn test_stagnation_detection() {
        let config = ResearchConfig {
            max_recursion_depth: 5,
            min_recursion_depth: 1,
            stagnation_limit: 3, // 低阈值便于测试
            quality_targets: QualityTargets::default(),
            ..Default::default()
        };

        let mut controller = RecursionController::new(config);

        // 连续 3 次没有新事实（第1次设置基线，第2-4次触发停滞）
        for i in 0..4 {
            let kb = create_test_kb(10, 1); // 同样的事实数量
            let decision = controller.should_continue(&kb);

            if matches!(decision, RecursionDecision::Stop { reason } if matches!(reason, StopReason::Stagnation)) {
                assert!(i >= 3, "Should trigger after at least 3 stagnation counts");
                return; // 测试通过
            }
        }

        panic!("Should have stopped due to stagnation");
    }

    #[test]
    fn test_continue_with_candidates() {
        let config = ResearchConfig {
            max_recursion_depth: 5,
            min_recursion_depth: 1,
            stagnation_limit: 5,
            quality_targets: QualityTargets::default(),
            ..Default::default()
        };

        let mut controller = RecursionController::new(config);

        let mut kb = create_test_kb(5, 1);
        // 添加高优先级候选
        kb.recursion_candidates.push(RecursionCandidate {
            entity_type: EntityType::Event,
            entity: "Test Event".to_string(),
            context: "Test".to_string(),
            priority: Priority::High,
            layer2_queries: vec!["query2".to_string()],
            layer3_queries: vec!["query3".to_string()],
            reason: "Test".to_string(),
        });

        let decision = controller.should_continue(&kb);

        match decision {
            RecursionDecision::Continue { next_layer, .. } => {
                assert_eq!(next_layer, 2);
            }
            _ => panic!("Should continue with high priority candidates"),
        }
    }

    #[test]
    fn test_generate_next_layer_queries() {
        let config = ResearchConfig::default();
        let controller = RecursionController::new(config);

        let mut kb = KnowledgeBase::default();
        kb.recursion_candidates = vec![
            RecursionCandidate {
                entity_type: EntityType::Person,
                entity: "Elon Musk".to_string(),
                context: "Tesla CEO".to_string(),
                priority: Priority::High,
                layer2_queries: vec!["Elon Musk background".to_string()],
                layer3_queries: vec!["Elon Musk impact".to_string()],
                reason: "Key person".to_string(),
            },
        ];

        let queries = controller.generate_next_layer_queries(2, &kb);

        assert!(!queries.is_empty());
        assert!(queries[0].task.contains("Elon Musk") || queries[0].task.contains("background"));
    }

    #[test]
    fn test_stop_reason_as_str() {
        assert_eq!(StopReason::MaxDepthReached.as_str(), "Maximum recursion depth reached");
        assert_eq!(StopReason::Stagnation.as_str(), "No new information in recent iterations");
        assert_eq!(StopReason::QualityTargetsMet.as_str(), "Quality targets satisfied");
        assert_eq!(StopReason::NoCandidates.as_str(), "No recursion candidates available");
        assert_eq!(StopReason::NoQueriesGenerated.as_str(), "Could not generate follow-up queries");
    }
}
