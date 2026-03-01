//! Synthesize research tool: reads deep_search source files and produces a
//! comprehensive analysis via map-reduce LLM calls.
//!
//! After `deep_search` saves source files to disk (up to 20K chars each),
//! this tool reads them all, batches into context-window-sized chunks,
//! extracts key findings per batch, then merges into a final synthesis.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use crew_core::{Message, MessageRole, TokenUsage};
use crew_llm::{ChatConfig, LlmProvider};
use eyre::{Result, WrapErr};
use serde::Deserialize;
use tracing::{info, warn};

use super::{Tool, ToolResult};

/// Maximum chars per LLM batch (~80K chars ≈ ~20K tokens).
const BATCH_CHAR_LIMIT: usize = 80_000;
/// Maximum total chars to process (safety cap).
const TOTAL_CHAR_LIMIT: usize = 500_000;
/// Maximum number of source files to read.
const MAX_FILES: usize = 50;

pub struct SynthesizeResearchTool {
    llm: Arc<dyn LlmProvider>,
    data_dir: PathBuf,
}

impl SynthesizeResearchTool {
    pub fn new(llm: Arc<dyn LlmProvider>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            llm,
            data_dir: data_dir.into(),
        }
    }

    /// Resolve research directory from whatever the LLM provides.
    /// Tries: exact path, relative to cwd, relative to data_dir, and just the slug under research/.
    fn resolve_research_dir(&self, input: &str) -> Option<PathBuf> {
        let stripped = input.trim().trim_start_matches("./");
        let cwd = std::env::current_dir().unwrap_or_default();

        // Extract the slug (last path component) for fuzzy matching
        let slug = std::path::Path::new(stripped)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(stripped);

        // Candidate directories to try, in priority order
        let candidates: Vec<PathBuf> = vec![
            // 1. Exact absolute path
            PathBuf::from(input),
            // 2. Relative to cwd (deep_search uses ./research/<slug>)
            cwd.join(stripped),
            // 3. Just slug under cwd/research/
            cwd.join("research").join(slug),
            // 4. Relative to data_dir
            self.data_dir.join(stripped),
            // 5. Just slug under data_dir/research/
            self.data_dir.join("research").join(slug),
        ];

        for candidate in candidates {
            if candidate.is_dir() {
                info!(resolved = %candidate.display(), input = %input, "resolved research directory");
                return Some(candidate);
            }
        }

        warn!(input = %input, "could not resolve research directory");
        None
    }

    /// Read all source .md files from a research directory.
    async fn read_sources(&self, dir: &PathBuf) -> Result<Vec<(String, String)>> {
        let mut entries = tokio::fs::read_dir(dir)
            .await
            .wrap_err_with(|| format!("cannot read directory: {}", dir.display()))?;

        let mut files = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // Skip non-markdown, index files, and report files
            if !name.ends_with(".md") {
                continue;
            }
            if name.starts_with('_') {
                continue;
            }

            if files.len() >= MAX_FILES {
                warn!(max = MAX_FILES, "reached file limit, skipping remaining");
                break;
            }

            match tokio::fs::read_to_string(&path).await {
                Ok(content) if !content.is_empty() => {
                    files.push((name, content));
                }
                Ok(_) => {} // skip empty files
                Err(e) => {
                    warn!(file = %name, error = %e, "failed to read source file");
                }
            }
        }

        // Sort by filename for deterministic ordering
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    /// Partition files into batches that fit within the char limit.
    fn partition_batches(files: &[(String, String)]) -> Vec<Vec<usize>> {
        let mut batches: Vec<Vec<usize>> = Vec::new();
        let mut current_batch: Vec<usize> = Vec::new();
        let mut current_size: usize = 0;

        for (i, (_name, content)) in files.iter().enumerate() {
            let size = content.len();
            if !current_batch.is_empty() && current_size + size > BATCH_CHAR_LIMIT {
                batches.push(std::mem::take(&mut current_batch));
                current_size = 0;
            }
            current_batch.push(i);
            current_size += size;
        }

        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        batches
    }

    /// Map phase: extract key findings from a batch of source files.
    async fn extract_findings(
        &self,
        query: &str,
        focus: Option<&str>,
        files: &[(String, String)],
        batch_indices: &[usize],
        batch_num: usize,
        total_batches: usize,
    ) -> Result<(String, TokenUsage)> {
        let mut sources = String::new();
        for &i in batch_indices {
            let (name, content) = &files[i];
            sources.push_str(&format!("### Source: {name}\n\n{content}\n\n---\n\n"));
        }

        let focus_instruction = match focus {
            Some(f) => format!("\n\nFocus particularly on: {f}"),
            None => String::new(),
        };

        let prompt = format!(
            "You are analyzing research sources (batch {batch_num}/{total_batches}).\n\n\
             Original research question: {query}{focus_instruction}\n\n\
             Extract ALL key findings from these sources. Rules:\n\
             - Keep ALL specific numbers, percentages, dates, names, and quotes\n\
             - Keep ALL source URLs and citations\n\
             - Organize findings by topic/theme\n\
             - Include contradictions or differing perspectives\n\
             - Be comprehensive — do not summarize away important details\n\n\
             Sources:\n\n{sources}"
        );

        let messages = vec![Message {
            role: MessageRole::User,
            content: prompt,
            media: vec![],
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
            timestamp: chrono::Utc::now(),
        }];

        let config = ChatConfig {
            max_tokens: Some(8192),
            temperature: Some(0.0),
            ..Default::default()
        };

        let response = self.llm.chat(&messages, &[], &config).await?;
        let usage = TokenUsage {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
        };
        Ok((response.content.unwrap_or_default(), usage))
    }

    /// Reduce phase: merge partial findings into a final synthesis.
    async fn merge_findings(
        &self,
        query: &str,
        focus: Option<&str>,
        partials: &[String],
        source_count: usize,
    ) -> Result<(String, TokenUsage)> {
        let mut sections = String::new();
        for (i, partial) in partials.iter().enumerate() {
            sections.push_str(&format!(
                "## Partial Analysis {}\n\n{}\n\n---\n\n",
                i + 1,
                partial
            ));
        }

        let focus_instruction = match focus {
            Some(f) => format!("\n\nFocus particularly on: {f}"),
            None => String::new(),
        };

        let prompt = format!(
            "Synthesize these {count} partial analyses into ONE comprehensive research report.\n\n\
             Original question: {query}{focus_instruction}\n\n\
             Rules:\n\
             - Remove duplicates and redundancies across partial analyses\n\
             - Organize logically with clear section headers (use ## and ###)\n\
             - Keep ALL specific numbers, percentages, dates, names, and direct quotes\n\
             - Use markdown tables where data comparison is appropriate\n\
             - Include a ## Sources section at the end listing all URLs referenced\n\
             - Note any contradictions or areas of disagreement between sources\n\
             - Write in the same language as the original question\n\n\
             Analyzed {source_count} source pages total.\n\n\
             {sections}\n\n\
             Write the complete synthesized report.",
            count = partials.len(),
        );

        let messages = vec![Message {
            role: MessageRole::User,
            content: prompt,
            media: vec![],
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
            timestamp: chrono::Utc::now(),
        }];

        let config = ChatConfig {
            max_tokens: Some(8192),
            temperature: Some(0.0),
            ..Default::default()
        };

        let response = self.llm.chat(&messages, &[], &config).await?;
        let usage = TokenUsage {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
        };
        Ok((response.content.unwrap_or_default(), usage))
    }
}

#[derive(Deserialize)]
struct Input {
    research_dir: String,
    query: String,
    #[serde(default)]
    focus: Option<String>,
}

#[async_trait]
impl Tool for SynthesizeResearchTool {
    fn name(&self) -> &str {
        "synthesize_research"
    }

    fn description(&self) -> &str {
        "Read all source files from a deep_search research directory and produce a comprehensive \
         synthesis using map-reduce analysis. This reads the FULL content of every saved source \
         page (up to 20K chars each) — much more thorough than the truncated previews returned \
         by deep_search. Use this after deep_search completes to get a detailed, data-rich report."
    }

    fn tags(&self) -> &[&str] {
        &["web"]
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "research_dir": {
                    "type": "string",
                    "description": "Path to the research directory from deep_search output (e.g. './research/topic-name' or 'research/topic-name')"
                },
                "query": {
                    "type": "string",
                    "description": "The original research question (provides context for synthesis)"
                },
                "focus": {
                    "type": "string",
                    "description": "Optional: specific aspect to focus the synthesis on"
                }
            },
            "required": ["research_dir", "query"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: Input =
            serde_json::from_value(args.clone()).wrap_err("invalid synthesize_research input")?;

        // Resolve research directory transparently.
        // deep_search saves to ./research/<slug> relative to cwd.
        // The LLM may pass any of: absolute path, ./research/slug, research/slug, or just the slug.
        let dir = match self.resolve_research_dir(&input.research_dir) {
            Some(d) => d,
            None => {
                return Ok(ToolResult {
                    output: "Research directory not found. Run deep_search first.".into(),
                    success: false,
                    ..Default::default()
                });
            }
        };

        info!(
            dir = %dir.display(),
            query = %input.query,
            focus = ?input.focus,
            "starting research synthesis"
        );

        // Step 1: Read all source files
        let files = self.read_sources(&dir).await?;

        if files.is_empty() {
            return Ok(ToolResult {
                output: format!(
                    "No source files found in {}. The research directory may be empty.",
                    dir.display()
                ),
                success: false,
                ..Default::default()
            });
        }

        let total_chars: usize = files.iter().map(|(_, c)| c.len()).sum();
        info!(
            file_count = files.len(),
            total_chars,
            "read source files"
        );

        // Truncate if over total limit
        let files: Vec<(String, String)> = if total_chars > TOTAL_CHAR_LIMIT {
            warn!(
                total_chars,
                limit = TOTAL_CHAR_LIMIT,
                "total content exceeds limit, truncating files"
            );
            let mut acc = 0usize;
            files
                .into_iter()
                .take_while(|(_, c)| {
                    acc += c.len();
                    acc <= TOTAL_CHAR_LIMIT
                })
                .collect()
        } else {
            files
        };

        let source_count = files.len();
        let mut total_tokens = TokenUsage::default();

        // Step 2: Partition into batches
        let batches = Self::partition_batches(&files);
        info!(
            batches = batches.len(),
            source_count,
            "partitioned into batches"
        );

        if batches.len() == 1 {
            // Single batch: direct synthesis (no map phase needed)
            info!("single batch — direct synthesis");
            let (synthesis, usage) = self
                .extract_findings(
                    &input.query,
                    input.focus.as_deref(),
                    &files,
                    &batches[0],
                    1,
                    1,
                )
                .await?;

            total_tokens.input_tokens += usage.input_tokens;
            total_tokens.output_tokens += usage.output_tokens;

            return Ok(ToolResult {
                output: format!(
                    "{synthesis}\n\n---\n_Synthesized from {source_count} source files._"
                ),
                success: true,
                tokens_used: Some(total_tokens),
                ..Default::default()
            });
        }

        // Step 3: Map phase — extract findings from each batch
        let total_batches = batches.len();
        let mut partials = Vec::with_capacity(total_batches);

        for (i, batch) in batches.iter().enumerate() {
            info!(
                batch = i + 1,
                total = total_batches,
                files_in_batch = batch.len(),
                "extracting findings from batch"
            );

            match self
                .extract_findings(
                    &input.query,
                    input.focus.as_deref(),
                    &files,
                    batch,
                    i + 1,
                    total_batches,
                )
                .await
            {
                Ok((findings, usage)) => {
                    total_tokens.input_tokens += usage.input_tokens;
                    total_tokens.output_tokens += usage.output_tokens;
                    if !findings.is_empty() {
                        partials.push(findings);
                    }
                }
                Err(e) => {
                    warn!(batch = i + 1, error = %e, "batch extraction failed");
                }
            }
        }

        if partials.is_empty() {
            return Ok(ToolResult {
                output: "All batch extractions failed. Could not synthesize research.".into(),
                success: false,
                tokens_used: Some(total_tokens),
                ..Default::default()
            });
        }

        // Step 4: Reduce phase — merge partials
        info!(
            partial_count = partials.len(),
            "merging partial analyses"
        );

        let (synthesis, merge_usage) = self
            .merge_findings(
                &input.query,
                input.focus.as_deref(),
                &partials,
                source_count,
            )
            .await?;

        total_tokens.input_tokens += merge_usage.input_tokens;
        total_tokens.output_tokens += merge_usage.output_tokens;

        Ok(ToolResult {
            output: format!(
                "{synthesis}\n\n---\n_Synthesized from {source_count} source files \
                 across {total_batches} batches._"
            ),
            success: true,
            tokens_used: Some(total_tokens),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_batches_single() {
        let files = vec![
            ("a.md".into(), "x".repeat(1000)),
            ("b.md".into(), "y".repeat(1000)),
        ];
        let batches = SynthesizeResearchTool::partition_batches(&files);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0], vec![0, 1]);
    }

    #[test]
    fn test_partition_batches_multiple() {
        let files: Vec<(String, String)> = (0..5)
            .map(|i| (format!("{i}.md"), "x".repeat(30_000)))
            .collect();
        let batches = SynthesizeResearchTool::partition_batches(&files);
        // 5 files × 30K = 150K, should split into ~2 batches (80K limit)
        assert!(batches.len() >= 2);
        // All indices should be covered
        let all: Vec<usize> = batches.iter().flatten().copied().collect();
        assert_eq!(all, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_partition_batches_empty() {
        let files: Vec<(String, String)> = vec![];
        let batches = SynthesizeResearchTool::partition_batches(&files);
        assert!(batches.is_empty());
    }

    #[test]
    fn test_partition_single_large_file() {
        let files = vec![("big.md".into(), "x".repeat(100_000))];
        let batches = SynthesizeResearchTool::partition_batches(&files);
        // Single file always gets its own batch even if over limit
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0], vec![0]);
    }
}
