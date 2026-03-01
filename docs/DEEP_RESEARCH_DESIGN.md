# Deep Research: Next-Gen Research Pipeline Design

> Status: Design Phase
> Date: 2026-03-01
> Codename: **Deep Research** (flagship offering)

## Background: Current "Deep Search" (v1)

Our current pipeline (`deep_search` + `synthesize_research`) is a **single-pass search-then-summarize** system:

1. LLM calls `deep_search` tool
2. Tool runs 3 search rounds, fetches ~30 pages, saves full content to disk
3. LLM calls `synthesize_research` which reads all files, does map-reduce (80K char batches), returns merged findings
4. LLM formats answer for user

**Limitations vs Gemini/Kimi:**

| Dimension | Our Deep Search v1 | Gemini Deep Research | Kimi K2.5 Agent Swarm |
|---|---|---|---|
| Search queries | ~10 keywords, 3 rounds | 80-160 queries | 1,500 tool calls |
| Pages read | ~30 | 100+ full pages | 206 URLs (top 3.2% retained) |
| Sub-agents | 0 | Orchestrator + parallel | Up to 100 parallel |
| Reflection/gap detection | None | Search → Reflect → Fill gaps loop | RL-learned iterative refinement |
| Synthesis | Single-pass map-reduce | Multi-pass self-critique | RL-optimized report generation |
| Quality filtering | None (all content equal) | Relevance scoring | Only top 3.2% retained |
| Output detail | ~6K chars (sketchy) | Comprehensive structured report | 10,000+ words with 26 citations |
| Time | ~6 min | 5-60 min | 3-5 min (parallel) |

The core problems:
1. **No reflection loop** — never asks "what's missing?" after searching
2. **No topic-aware decomposition** — single generic query, not specialized per angle/language/region
3. **Synthesis = summarization** — map-reduce extracts then merges, no self-critique or quality filtering

---

## Design: Deep Research (Next-Gen)

### Three-Phase Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Phase 1: PLANNING                        │
│                                                             │
│  User query → Orchestrator LLM generates research plan:     │
│  - Decompose into N topic angles (5-20, not fixed at 3)     │
│  - Per angle: target languages, regions, site types          │
│  - Present plan to user for review/modification              │
│                                                             │
│  Example: "Iran succession crisis"                          │
│  ├─ Persian/Farsi news (local Iranian portals)              │
│  ├─ Arabic news (Al Jazeera, Al Arabiya)                    │
│  ├─ Israeli analysis (Haaretz, Times of Israel)             │
│  ├─ Western analysis (Reuters, BBC, NYT)                    │
│  ├─ Academic/think tank (RAND, Brookings, IISS)             │
│  └─ Social media / OSINT angle                              │
│                                                             │
│  Example: "2026 World Cup predictions"                      │
│  ├─ Spanish sports press (Marca, AS, Mundo Deportivo)       │
│  ├─ Portuguese sports (A Bola, Record)                      │
│  ├─ English sports (BBC Sport, The Athletic, ESPN)          │
│  ├─ Latin American coverage (Ole, Globo Esporte)            │
│  ├─ Statistical/analytics (FBref, Opta, 538)               │
│  ├─ Betting/prediction markets (Betfair, Polymarket)        │
│  └─ FIFA/official sources                                   │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│              Phase 2: COLLECTION (Parallel)                 │
│                                                             │
│  5-20 sub-agents run concurrently, each:                    │
│                                                             │
│  ┌─────────────────────────────────────┐                    │
│  │  Sub-Agent N (specialized angle)    │                    │
│  │                                     │                    │
│  │  loop {                             │                    │
│  │    1. Generate targeted queries     │                    │
│  │       (language-specific, site-     │                    │
│  │        specific, region-aware)      │                    │
│  │    2. Search + browse full pages    │                    │
│  │    3. REFLECT:                      │                    │
│  │       - Score relevance (0-10)      │                    │
│  │       - Discard noise (keep top N%) │                    │
│  │       - Detect gaps: "what's        │                    │
│  │         missing for this angle?"    │                    │
│  │    4. If gaps → generate new        │                    │
│  │       queries → loop               │                    │
│  │    5. If sufficient → write         │                    │
│  │       findings to partial_N.md      │                    │
│  │       with citations                │                    │
│  │  }                                  │                    │
│  └─────────────────────────────────────┘                    │
│                                                             │
│  Wall-time = slowest sub-agent (parallel execution)         │
│  Each sub-agent: 3-5 search-reflect cycles                  │
│  Quality gate: relevance filter before synthesis            │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│          Phase 3: SYNTHESIS (Fresh Context)                 │
│                                                             │
│  Brand new LLM session (no search history burden):          │
│                                                             │
│  1. Read ALL partial findings from sub-agents               │
│  2. Cross-reference across angles:                          │
│     - Detect contradictions between sources                 │
│     - Find consensus across regions/languages               │
│     - Identify unique insights per angle                    │
│  3. Generate structured draft report                        │
│  4. SELF-CRITIQUE:                                          │
│     - Review draft for weak sections                        │
│     - Check citation coverage                               │
│     - Identify remaining gaps                               │
│     - If critical gaps → can request additional search      │
│  5. Final report: comprehensive, data-rich, cited           │
│                                                             │
│  Output: Canonical Markdown report                          │
│  ├─ Can convert to PPTX (existing skill)                    │
│  ├─ Can convert to DOCX (future)                            │
│  ├─ Can convert to website/infographic (future)             │
│  └─ Stored as artifact for future reference                 │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

1. **Dynamic sub-agent count**: Orchestrator decides how many angles based on query complexity (simple factual → 3, geopolitical analysis → 15+). Not hardcoded.

2. **Language/region-aware queries**: The plan explicitly specifies which languages and regional sources to target. A query about Iran should search Farsi, Arabic, Hebrew, and English sources — not just English.

3. **Reflection loop per sub-agent**: Each sub-agent runs search → reflect → search more if gaps detected. 3-5 cycles. This is what makes output comprehensive vs sketchy.

4. **Quality filtering**: Each sub-agent scores content relevance and discards noise before synthesis. Aim for Kimi's ~3-5% retention rate. 200 pages fetched → 7-10 high-quality pages retained.

5. **Fresh synthesis context**: Synthesis agent starts with zero history. Only receives curated findings from collection phase. Prevents context pollution from search process noise.

6. **Report as artifact**: Markdown is the canonical output format. Downstream conversion to PPTX/DOCX/website is a separate concern, not part of the research pipeline.

7. **Self-critique in synthesis**: The synthesis agent reviews its own draft, identifies weak sections, and can request targeted additional searches before finalizing.

---

## Why RL Training Matters (and Why We Don't Need It Yet)

### What RL Training Does for Gemini/Kimi

Kimi-Researcher achieved its results through **end-to-end reinforcement learning** — the model itself learned agentic behaviors (when to search, what queries to generate, when to stop, what to keep) through reward signals, not prompt engineering.

Their RL reward structure:
- **Format reward**: penalizes invalid tool calls or budget overruns
- **Correctness reward**: compares output against verified ground truth
- **Efficiency reward**: `r * gamma^(T-i)` — encourages shorter trajectories (find answers quickly)

Starting from 8.6% on HLE (Humanity's Last Exam), Kimi reached 26.9% through RL alone. The model learned:
- When to issue follow-up searches (gap detection is *emergent*, not programmed)
- What search queries are most productive (74 keywords avg, learned distribution)
- What content to keep vs discard (3.2% retention rate, learned filter)
- When enough information has been gathered (learned stopping criterion)

### Why RL Matters: Prompt Engineering Has a Ceiling

**Prompt-based approach** (what we can do now):
```
"Search for X. After searching, reflect on what gaps remain.
If gaps exist, search again with targeted queries. Repeat 3-5 times."
```

This *works* but has fundamental limitations:

1. **Search query quality**: A prompted model generates "reasonable" queries. An RL-trained model has optimized over millions of trajectories to find queries that *actually produce useful results*. The difference is like a chess player who knows the rules vs one who has played 10 million games.

2. **Stopping criterion**: When should the agent stop searching? A prompted agent uses a fixed rule ("3-5 cycles") or vague judgment. An RL agent has learned the optimal stopping point — the point where additional searches have diminishing returns. It's been rewarded for efficiency.

3. **Quality filtering**: "Keep only relevant content" in a prompt is subjective. An RL agent has learned *exactly* what "relevant" means for producing high-scoring reports — through thousands of training examples with ground truth.

4. **Exploration strategy**: A prompted agent explores uniformly. An RL agent has learned to allocate more search budget to harder sub-topics and less to easy ones. It has a *learned policy* for resource allocation.

5. **Compounding errors**: In a 23-step trajectory (Kimi's average), each step's quality affects the final output. Prompt engineering optimizes each step independently. RL optimizes the *entire trajectory end-to-end*, accounting for how early decisions affect later steps.

### What This Means in Practice

| Capability | Prompt-Based (Us) | RL-Trained (Kimi/Gemini) |
|---|---|---|
| Query generation | Good, generic | Optimized for information yield |
| Gap detection | Explicit "check for gaps" prompt | Emergent from reward signal |
| When to stop searching | Fixed rules (3-5 cycles) | Learned optimal stopping |
| Content filtering | Heuristic or LLM judgment | Learned relevance model |
| Resource allocation | Uniform across sub-topics | Adaptive (more budget for harder topics) |
| Report quality | Good (well-prompted) | Excellent (optimized for report reward) |
| Error compounding | Accumulates over steps | Minimized via trajectory optimization |

### Our Approach: Architecture First, RL Later

**Phase 1 (Now)**: Build the right *architecture* with prompt-based agents:
- Orchestrator + parallel sub-agents + reflection loops + fresh synthesis
- This alone will be a massive improvement over single-pass search-summarize
- Estimated improvement: from sketchy ~6K char summaries to structured ~10K+ word reports

**Phase 2 (Future)**: Add RL training on top of the architecture:
- Collect trajectories from production usage (search logs, user feedback)
- Define reward: report quality (accuracy, completeness, citation correctness)
- Train the orchestrator and sub-agents via REINFORCE/PPO
- This is where we'd close the remaining gap to Kimi/Gemini

The architecture is the prerequisite. Kimi's RL wouldn't work without the right tool interface and agent loop. We build that first.

---

## Implementation Plan

### New Components

1. **`DeepResearchOrchestrator`** (new tool in `crew-agent/src/tools/`)
   - Takes user query + optional configuration
   - Phase 1: Calls LLM to generate research plan (N angles with metadata)
   - Phase 2: Spawns N sub-agents in parallel (reuses existing `Agent` infrastructure)
   - Phase 3: Collects partials, spawns fresh synthesis agent
   - Returns final report + metadata

2. **`ResearchSubAgent`** (configuration for collection phase agents)
   - Specialized system prompt per angle
   - Tools: `web_search`, `web_fetch`, `browser`, `deep_search`, `read_file`, `write_file`
   - Reflection loop built into system prompt
   - Quality filter: score + discard low-relevance content
   - Max iterations: configurable per angle complexity

3. **`SynthesisAgent`** (configuration for final synthesis)
   - Fresh context (no search history)
   - Long context model preferred (Gemini 2.5 Pro 1M, or largest available)
   - Self-critique loop: draft → review → revise
   - Citation validation pass

### Files to Modify

- `crates/crew-agent/src/tools/deep_research_v2.rs` — new orchestrator
- `crates/crew-agent/src/tools/mod.rs` — register new tool
- `crates/crew-agent/src/prompts/research_planner.txt` — orchestrator planning prompt
- `crates/crew-agent/src/prompts/research_collector.txt` — sub-agent collection prompt
- `crates/crew-agent/src/prompts/research_synthesizer.txt` — synthesis prompt
- `crates/crew-cli/src/commands/gateway.rs` — register tool, update system prompt
- `crates/crew-cli/src/commands/chat.rs` — register tool

### Naming

| Component | Name | Description |
|---|---|---|
| Current pipeline | `deep_search` + `synthesize_research` | Single-pass search + map-reduce synthesis |
| Next-gen pipeline | `deep_research` (tool name) | Multi-agent orchestrated research with reflection |
| User-facing brand | **Deep Research** | Flagship research capability |

The existing `DeepResearchTool` will be retired or renamed to avoid confusion. The new tool takes over the `deep_research` name.

---

## Success Metrics

- **Report length**: from ~6K chars to 10K+ words
- **Citation count**: from ~0 (inline only) to 20+ traceable citations
- **Source diversity**: from single-language to multi-language/multi-region
- **User satisfaction**: reports should contain specific data points, numbers, dates, quotes — not generic summaries
- **Time budget**: 5-15 minutes (acceptable for comprehensive research)

## References

- [Gemini Deep Research API](https://ai.google.dev/gemini-api/docs/deep-research)
- [Google LangGraph Reference Implementation](https://github.com/google-gemini/gemini-fullstack-langgraph-quickstart)
- [Kimi-Researcher Technical Report](https://moonshotai.github.io/Kimi-Researcher/)
- [Kimi K2.5 Technical Report](https://arxiv.org/html/2602.02276v1)
- [DeepSearchQA Benchmark](https://www.arxiv.org/pdf/2601.20975)
- [ByteByteGo: How OpenAI, Gemini, and Claude Use Agents for Deep Research](https://blog.bytebytego.com/p/how-openai-gemini-and-claude-use)
