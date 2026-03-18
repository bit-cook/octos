---
name: mofa-research-2.0
description: "Deep research pipeline with dynamic parallel search, inspired by DeerFlow and mofa-research. Triggers: deep research, 深度研究, comprehensive research, research report, 研究报告"
requires_bins: curl
requires_env: TAVILY_API_KEY
always: false
---

# MOFA Research 2.0

A deep research pipeline that combines DeerFlow's iterative exploration with mofa-research's dynamic parallel architecture.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         MOFA RESEARCH 2.0 PIPELINE                          │
└─────────────────────────────────────────────────────────────────────────────┘

Phase 1: PLAN (Strong Model)
─────────────────────────────
Lead Agent analyzes query → Generates 4-8 dynamic search angles
                                          ↓
                              ┌─────────────────────┐
                              │  JSON Output:       │
                              │  [{task, label},    │
                              │   {task, label},    │
                              │   ...]              │
                              └─────────────────────┘

Phase 2: DISCOVERY (Parallel Workers)
──────────────────────────────────────
                         ┌─────────────┐
                    ┌────┤  Worker 1   ├────┐
                    │    └─────────────┘    │
                    │    ┌─────────────┐    │
              Spawn ├────┤  Worker 2   ├────┤ Parallel
              All   │    └─────────────┘    │ Execution
              at    │    ┌─────────────┐    │
              Once  ├────┤  Worker 3   ├────┤
                    │    └─────────────┘    │
                    │         ...           │
                    │    ┌─────────────┐    │
                    └────┤  Worker N   ├────┘
                         └─────────────┘

Each Worker:
- Has ISOLATED context (only sees its own task)
- Runs Tavily search with time-filtered queries
- Extracts facts, sources, follow-up leads
- Returns structured output

Phase 3: MERGE
──────────────
All worker outputs combined with:
- ## Label headers
- --- separators
- → KnowledgeBase (merged_outputs.md)

Phase 4: ANALYZE (Strong Model)
───────────────────────────────
Cross-reference all findings:
┌─────────────────────────────────────────────────────────┐
│  • Identify key facts across ALL angles                 │
│  • Cross-reference information                          │
│  • Filter by RECENCY (prioritize last 30 days)          │
│  • Organize by subtopic                                 │
│  • Rate source credibility                              │
│  • Flag contradictions                                  │
│  • Identify gaps                                        │
└─────────────────────────────────────────────────────────┘
                              ↓
                    → Analysis Report (analysis.md)

Phase 5: SYNTHESIZE (Strong Model + Goal Gate)
──────────────────────────────────────────────
Generate comprehensive report:
┌─────────────────────────────────────────────────────────┐
│  • Executive Summary                                    │
│  • Key Findings (5-8 detailed)                          │
│  • Detailed Analysis                                    │
│  • Contradictions & Uncertainties                       │
│  • Full Source List                                     │
│                                                         │
│  MANDATORY: Save to ./research/{slug}/report.md         │
│  MINIMUM: 8000 characters                               │
│  NO EMOJI                                               │
└─────────────────────────────────────────────────────────┘

Phase 6: RECURSION (Optional Deep Dive)
────────────────────────────────────────
If gaps identified or topics need deeper exploration:
                          ↓
              ┌─────────────────────┐
              │ Generate Sub-Angles │
              │ for Each Gap        │
              └──────────┬──────────┘
                         ↓
              ┌─────────────────────┐
              │ Spawn Sub-Workers   │
              │ (Layer 2 Search)    │
              └──────────┬──────────┘
                         ↓
              ┌─────────────────────┐
              │ Background/Impact   │
              │ Analysis            │
              └─────────────────────┘
```

## Time-Aware Research (CRITICAL)

DeerFlow-inspired temporal precision:

| User Intent | Temporal Precision | Example Query |
|-------------|-------------------|---------------|
| "today / this morning" | **Month + Day** | `"tech news March 11 2026"` |
| "this week" | Week range | `"AI releases week of March 9 2026"` |
| "recently / latest" | Month | `"AI breakthroughs March 2026"` |
| "this year" | Year | `"tech trends 2026"` |

**Rules:**
- ALWAYS check current date before forming queries
- Never use year-only when day-level precision is needed
- Try multiple phrasings: numeric (`2026-03-11`), written (`March 11 2026`)
- Include time filters in Tavily queries

## Phase 1: Plan (Dynamic Angle Generation)

**Model:** Strong (for planning quality)

**Planning Prompt:**

```json
{
  "role": "user",
  "content": "Generate 4-8 research search angles for this query. Each angle should cover a different aspect — do NOT just rephrase the same query.\n\nQuery: {{QUERY}}\nCurrent Date: {{CURRENT_DATE}}\n\nRequirements:\n- 4-8 distinct angles covering: core topic, alternatives/comparisons, technical details, recent developments, market impact, regional differences, expert opinions, challenges/limitations\n- Include at least one angle in Chinese and one in English\n- Use SPECIFIC temporal filters based on current date\n- Each task should be executable as a standalone search\n\nRespond with ONLY a JSON array:\n[\n  {\"task\": \"Search specific query with date filter\", \"label\": \"Descriptive Label\"},\n  ...\n]"
}
```

**Fallback:** If planner fails or returns <2 tasks, use:
```json
[
  {"task": "{{query}} latest news {{current_year}}", "label": "Latest News"},
  {"task": "{{query}} developments {{current_month}} {{current_year}}", "label": "Recent Developments"},
  {"task": "搜 {{query}} 最新消息 {{current_year}}", "label": "中文新闻"}
]
```

## Phase 2: Discovery (Isolated Parallel Workers)

**Model:** Fast/Cheap (for parallel execution)

**Worker Isolation:** Each worker ONLY sees its assigned task, not other angles.

**Worker Execution:**

```bash
# Worker Prompt (ISOLATED):
"You are a research specialist focused on: {label}

Your task: {task}
Current date: {current_date}

Execute Tavily search, then extract:
1. Key facts with dates
2. Important quotes
3. Source URLs
4. Follow-up leads for deeper exploration

Return structured JSON output."
```

**Tavily Search with Time Filtering:**

```bash
# ALWAYS include temporal keywords in query
curl -s "https://api.tavily.com/search" \
  -H "Content-Type: application/json" \
  -d '{
    "api_key": "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",
    "query": "AI breakthrough March 11 2026",  # <-- Time-specific!
    "search_depth": "advanced",
    "max_results": 10
  }'
```

**Parallel Execution:**

```bash
# Spawn all workers concurrently
spawn "research-worker-1" "Worker 1 prompt with isolated context" &
spawn "research-worker-2" "Worker 2 prompt with isolated context" &
spawn "research-worker-3" "Worker 3 prompt with isolated context" &
spawn "research-worker-4" "Worker 4 prompt with isolated context" &
wait
```

**Worker Output Format (Structured JSON):**

```json
{
  "angle_label": "Label from planning",
  "query_executed": "Actual search query used",
  "execution_timestamp": "ISO 8601 timestamp",
  "results": [
    {
      "rank": 1,
      "title": "...",
      "url": "...",
      "content": "Summary...",
      "published_date": "2026-03-10",
      "source_type": "news|blog|academic|official",
      "authority_score": 0.8,
      "key_facts": [
        {
          "fact": "Specific claim",
          "quote": "Exact text from source",
          "confidence": "high|medium|low"
        }
      ]
    }
  ],
  "follow_up_leads": [
    {
      "type": "event|person|company|technology",
      "entity": "Name",
      "reason_to_explore": "Why this matters",
      "suggested_queries": ["...", "..."]
    }
  ],
  "coverage_assessment": {
    "satisfied": true|false,
    "gaps": ["..."]
  }
}
```

## Phase 2.5: Worker Failure Handling (容错机制)

**CRITICAL: Always handle worker failures gracefully**

After workers complete (success or failure):

### Failure Recovery

```bash
# Check which workers completed
completed_workers = []
failed_workers = []

for worker in spawned_workers:
    if worker.status == "completed":
        completed_workers.append(worker)
    elif worker.status == "failed" or worker.status == "timeout":
        failed_workers.append(worker)
        # Capture partial results if available
        if worker.partial_output:
            completed_workers.append(worker)

# Log failure summary
write_file "./research/{query-slug}/kb/execution_log.md" "Execution Summary:
- Total angles: {total}
- Successful: {len(completed_workers)}
- Failed: {len(failed_workers)}
- Failed angles: {failed_angles_list}"
```

### Minimum Viable Research

**Even if only 1 worker succeeds, continue to Merge phase.**

Requirements adjusted based on success rate:

| Success Rate | Min Angles | Min Sources | Min Facts | Report Length |
|-------------|-----------|-------------|-----------|---------------|
| 100% (4/4)  | 4         | 15          | 25        | 8000 chars    |
| 75% (3/4)   | 3         | 12          | 20        | 6000 chars    |
| 50% (2/4)   | 2         | 8           | 15        | 5000 chars    |
| 25% (1/4)   | 1         | 5           | 10        | 3000 chars    |

**Always generate report with caveats:**
> "Note: Based on {N} of {total} planned research angles due to timeouts. Coverage may be incomplete."

## Phase 3: Merge (KnowledgeBase Construction - ALWAYS EXECUTE)

**MANDATORY: This phase ALWAYS runs, regardless of worker failures.**

Combine all worker outputs (complete and partial):

```markdown
# Merged Research Outputs
Query: {original_query}
Generated: {timestamp}
Execution Summary:
- Total angles planned: {N}
- Successfully completed: {M}
- Failed/timeout: {N-M}
- Angles included in this merge: {list}

---

## Angle 1: {label} [STATUS: completed|partial|failed]
Query: {task}

{Worker 1 output OR failure reason}

---

## Angle 2: {label} [STATUS: completed|partial|failed]
Query: {task}

{Worker 2 output OR failure reason}

---

[Continue for all angles...]

## Cross-Reference Matrix

| Fact | Sources | Confidence |
|------|---------|------------|
| ...  | ...     | ...        |

## Coverage Gaps (from failed angles)

- {Angle X}: Failed due to timeout - potential gap in {topic}
- {Angle Y}: Failed due to timeout - potential gap in {topic}
```

Save to: `./research/{query-slug}/kb/merged_outputs.md`

## Phase 4: Analyze (Cross-Reference)

**Model:** Strong

**Analysis Prompt:**

```json
{
  "role": "user",
  "content": "You are a research analyst. Analyze the following merged search results from multiple parallel angles.\n\n[Read from: ./research/{query-slug}/kb/merged_outputs.md]\n\nYour task:\n1. Extract all key facts, data points, and insights from ALL angles\n2. Cross-reference: identify where sources agree or disagree\n3. TEMPORAL FILTERING: Prioritize facts from last 30 days\n4. Organize by subtopic, merging related information\n5. Rate source credibility (official > academic > news > blog)\n6. Flag contradictions explicitly\n7. Identify coverage gaps needing deeper exploration\n\nOutput: Structured analysis with:\n- Facts by subtopic\n- Contradictions table\n- Source credibility ratings\n- Gap analysis\n- Recommendations for recursion (if needed)"
}
```

Save to: `./research/{query-slug}/kb/analysis.md`

## Phase 5: Synthesize (Report Generation) - ALWAYS EXECUTE

**Model:** Strong

**Goal Gate:** MUST write file before completion.

**IMPORTANT: This phase ALWAYS runs, even if all workers failed. Generate report based on available data.**

**Adaptive Requirements (based on worker success rate):**

| Success Rate | Min Findings | Min Sources | Min Report Length |
|-------------|--------------|-------------|-------------------|
| 100% (4/4)  | 5-8          | 15          | 8000 chars        |
| 75% (3/4)   | 4-6          | 10          | 6000 chars        |
| 50% (2/4)   | 3-5          | 8           | 4000 chars        |
| 25% (1/4)   | 2-4          | 5           | 2500 chars        |
| 0% (0/4)    | 1-2          | 0           | 1000 chars (note) |

**Synthesis Prompt:**

```json
{
  "role": "user",
  "content": "You are a research synthesis expert. Produce a comprehensive report.\n\n[Read from: ./research/{query-slug}/kb/analysis.md]\n\nFirst, check execution summary in merged_outputs.md to determine success rate and adjust your approach.\n\nReport Requirements:\n\n1. EXECUTIVE SUMMARY (2-8 sentences based on available data)\n   - Key finding (or 'Limited data available' if workers failed)\n   - Methodology summary (note any timeouts/failures)\n   - Overall confidence\n\n2. KEY FINDINGS (see adaptive table for minimum count)\n   Each finding MUST include:\n   - Clear title\n   - Detailed explanation (not just headline)\n   - Specific numbers/dates/quotes\n   - Citations [n]\n   - Confidence level\n\n3. DETAILED ANALYSIS\n   - Organized by subtopic\n   - Comprehensive coverage (or note gaps if data is limited)\n\n4. CONTRADICTIONS & UNCERTAINTIES (if any)\n\n5. COVERAGE LIMITATIONS (MANDATORY if workers failed)\n   > Note: This report is based on X of Y planned research angles due to timeouts. Coverage may be incomplete.\n\n6. SOURCES\n   - Full bibliography\n   - [n] Title - URL (Type, Date, Authority)\n\nCRITICAL RULES:\n- Adjust length based on available data (see adaptive table)\n- Every major claim has citation\n- NO EMOJI - plain text only\n- Match query language\n- Include specific data, not generalizations\n- ALWAYS include coverage limitations section if any workers failed\n\nFINAL STEP: Use write_file to save to ./research/{query-slug}/report.md"
}
```

## Phase 5.5: Memory Persistence (mofa-memory integration)

This phase completes the `ResearchMemory` node shown in `architecture.dot`. It has no extra compute cost — it calls `mofa-memory` which runs in milliseconds (SQLite read/write) after the embedding API call.

### Pre-Research Recall (before Phase 1)

Before generating research angles, call `retrieve_memory` to surface any prior research on the same topic:

```json
{"query": "{{ORIGINAL_USER_QUERY}}", "top_k": 3, "min_score": 0.75}
```

If results are returned with `score >= 0.75`, present them to the user:

> "I found prior research on this topic from {created_at} (relevance: {score}). Shall I build on it or start fresh?"

If the user wants to build on it, include the prior report content in the Entry Agent's context for Phase 1.

### Post-Synthesis Persistence (after Phase 5)

After the report is written to `./research/{slug}/report.md`, persist it to long-term memory.

**Persist the final report:**
```json
{
  "content": "<full text read from ./research/{query-slug}/report.md>",
  "tags": ["{query-slug}", "{primary-topic}", "research-report"],
  "source": "./research/{query-slug}/report.md"
}
```

**Persist the knowledge base:**
```json
{
  "content": "<full text read from ./research/{query-slug}/kb/merged_outputs.md>",
  "tags": ["{query-slug}", "{primary-topic}", "kb-cache"],
  "source": "./research/{query-slug}/kb/merged_outputs.md"
}
```

**Requires:** `mofa-memory` skill installed and `OPENAI_API_KEY` available.
**Skip silently** if `mofa-memory` binary is not found — memory is optional, research always completes.

---

## User Output (Simplified)

**During research, ONLY show user:**

```
正在研究: {query}
├── 角度1: {label} [完成]
├── 角度2: {label} [超时]
├── 角度3: {label} [完成]
└── 角度4: {label} [完成]

正在交叉验证... 完成

正在生成报告... 完成

报告已保存: ./research/{query-slug}/report.md
```

**DO NOT show:**
- Internal JSON structures
- Raw search results
- Individual worker outputs
- File paths for intermediate files (merged_outputs.md, analysis.md)
- Detailed error traces

**DO show:**
- High-level progress (phase names only)
- Success/failure counts
- Final report location and summary

## Phase 6: Recursion (Deep Dive)

If analysis identifies gaps or high-priority follow-ups:

```
Gap Identified
      ↓
Generate Sub-Angles (2-4 focused queries)
      ↓
Spawn Sub-Workers (Layer 2)
      ↓
Background/Impact Analysis
      ↓
Merge with Layer 1 Results
      ↓
Updated Analysis
```

**Recursion Triggers:**
- Important claim with single source
- Contradiction between sources
- Event without background context
- Technology without impact assessment

## Output Requirements (MANDATORY)

### Files Must Be Written

1. **Merged Outputs** → `./research/{query-slug}/kb/merged_outputs.md`
2. **Analysis** → `./research/{query-slug}/kb/analysis.md`
3. **Final Report** → `./research/{query-slug}/report.md`

### Minimum Requirements

| Metric | Minimum | Target |
|--------|---------|--------|
| Search Angles | 4 | 6-8 |
| Sources | 15 | 25+ |
| Facts | 25 | 40+ |
| Report Length | 8000 chars | 12000+ |
| Recursion Depth | Layer 1 | Layer 2-3 |
| Citations | 10 | 15+ |

### Prohibited

- NO EMOJI in any output
- NO skipping file writes
- NO reports under 8000 characters
- NO single-source claims without verification

## Configuration

See `config.toml` for:
- `min_angles`: 4
- `max_workers`: 8
- `min_report_length`: 8000
- `recursion_enabled`: true
- `temporal_awareness`: strict

## Performance

With 4-8 parallel workers:
- **Plan**: ~30s
- **Discovery**: ~60s (parallel bottleneck)
- **Analyze**: ~90s
- **Synthesize**: ~60s
- **Total**: ~4-5 minutes for comprehensive research

## Comparison: mofa-research vs 2.0

| Feature | mofa-research | mofa-research-2.0 |
|---------|---------------|-------------------|
| Architecture | DOT pipeline | DOT + DeerFlow hybrid |
| Worker Isolation | No | Yes (isolated contexts) |
| Time Awareness | Basic | Strict (DeerFlow-inspired) |
| Recursion | Static | Dynamic (gap-driven) |
| Output Format | Simple | Docling-inspired structured |
| Plan Mode | No | TodoList-ready |

## Related Skills

### mofa-memory

Persistent vector memory across agent runs. Stores research reports in SQLite with OpenAI embeddings for semantic recall in future sessions.

**Use with mofa-research-2.0 when:**
- Running repeated research on related topics (avoids redundant work)
- Building a knowledge base over multiple research sessions
- Wanting to detect prior research before starting a new investigation

**Integration pattern:**
```
mofa-memory: retrieve_memory(query) → surface prior research (optional)
      ↓
mofa-research-2.0: Plan → Discover → Analyze → Synthesize → Report
      ↓
mofa-memory: store_memory(report + kb) → persist for future sessions
```

### mofa-crawler
Web crawling via Cloudflare Browser Rendering API for data acquisition.

**Use with mofa-research-2.0 when:**
- Target data is on specific websites not indexed by search engines
- Need full-site extraction (e.g., documentation, knowledge bases)
- Require JavaScript-rendered content (SPAs, React/Vue sites)
- Want structured data extraction with AI

**Integration pattern:**
```
mofa-crawler: Crawl docs site → Markdown
      ↓
mofa-research-2.0: Analyze + Synthesize → Research report
```

### mofa-firecrawl
Advanced web crawling and scraping via Firecrawl CLI with support for single-page scraping, full-site crawling, URL mapping, web search, and cloud browser automation.

**Use with mofa-research-2.0 when:**
- Need comprehensive site crawling with structured output
- Require dynamic content extraction with JavaScript rendering
- Want to combine web search with content scraping
- Need browser automation (clicking, form filling, navigation)
- Target sites require advanced extraction capabilities

**Integration pattern:**
```
mofa-research-2.0: Plan research angles
      ↓
mofa-firecrawl: Crawl target sites + Search supplementary sources
      ↓
mofa-research-2.0: Cross-reference + Analyze + Synthesize
```

**Feature comparison:**
| Feature | mofa-crawler | mofa-firecrawl |
|---------|--------------|----------------|
| Single page scrape | ✓ | ✓ |
| Full-site crawl | ✗ | ✓ |
| URL mapping | ✗ | ✓ |
| Web search | ✗ | ✓ |
| Browser automation | ✗ | ✓ |
| Cloud browser sessions | ✗ | ✓ |
| Self-hosted option | ✗ | ✓ |

### mofa-verge-browser
GUI browser sandbox automation with real visual Chromium, supporting CDP automation, GUI screenshots, and human-in-the-loop intervention.

**Use with mofa-research-2.0 when:**
- Target sites require visual interaction (captcha, MFA, complex authentication)
- Need human verification or intervention during research
- Want visual evidence of page states during research
- Researching sites with anti-bot measures that need real browser behavior
- Need to handle popups, downloads, or multi-tab scenarios

**Integration pattern:**
```
mofa-research-2.0: Plan research angles
      ↓
mofa-verge-browser: Visual browser automation + human intervention when needed
      ↓
mofa-research-2.0: Analyze visual evidence and extracted data
```

**Feature comparison with mofa-firecrawl:**
| Feature | mofa-firecrawl | mofa-verge-browser |
|---------|---------------|-------------------|
| Hosting | Cloud service | Self-hosted (local/Docker) |
| GUI browser | ✗ | ✓ (Real Chromium with GUI) |
| Visual screenshot | Basic | GUI-level with browser UI |
| Human intervention | ✗ | ✓ (noVNC/Xpra sessions) |
| CDP/Playwright | Limited | Full WebSocket CDP support |
| Captcha handling | ✗ | ✓ (with human-in-the-loop) |
| Ideal for | Large-scale crawling | Complex auth, visual tasks |

### mofa-pinchtab
Lightweight AI browser control via HTTP API with token-efficient text extraction (~800 tokens/page) and multi-instance parallel processing.

**Use with mofa-research-2.0 when:**
- Need token-efficient content extraction (cheaper than screenshots by 5-13x)
- Want lightweight local browser control without Docker complexity
- Need parallel multi-instance processing for batch URL extraction
- Require fast HTTP API-based automation

**Integration pattern:**
```
mofa-research-2.0: Plan research angles
      ↓
mofa-pinchtab: Parallel extraction of content from multiple sources
      ↓
mofa-research-2.0: Analyze extracted text → Research report
```

**Feature comparison:**
| Feature | mofa-firecrawl | mofa-verge-browser | mofa-pinchtab |
|---------|---------------|-------------------|---------------|
| Hosting | Cloud | Self-hosted (Docker) | Local binary |
| Setup complexity | Low | High | Low |
| Token efficiency | Medium | Low | **High** (~800t/page) |
| Multi-instance | ✗ | ✓ | ✓ |
| GUI browser | ✗ | ✓ | ✗ |
| Human intervention | ✗ | ✓ | ✗ |
| Best for | Scale | Complex auth | **Token-efficient extraction** |

### mofa-xhs
Xiaohongshu (小红书) integration for social media research.

**Use with mofa-research-2.0 when:**
- Researching Chinese consumer trends and opinions
- Analyzing product reviews and user experiences
- Tracking influencer content and engagement
- Studying fashion/beauty/travel trends

**Integration pattern:**
```
mofa-xhs: Search topics → Extract notes
      ↓
mofa-research-2.0: Cross-reference + Analyze → Trend report
```

### mofa-public-apis
Public API discovery for accessing external data sources.

**Use with mofa-research-2.0 when:**
- Need weather, finance, news data for research
- Looking for free data sources without API keys
- Building data enrichment pipelines

### mofa-crawlee-python
Web scraping with Crawlee-Python library for structured data extraction.

**Use with mofa-research-2.0 when:**
- Need to crawl and extract data from websites
- Target sites have JavaScript-heavy content (SPAs)
- Require robust scraping with retries and proxy rotation
- Want exportable datasets (JSON, CSV, Parquet)

**Integration pattern:**
```
mofa-research-2.0: Identify data sources
      ↓
mofa-crawlee-python: Crawl and extract structured data
      ↓
mofa-research-2.0: Analyze extracted data → Research report
```

