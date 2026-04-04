# Octos Architecture — The Complete Picture

## What Is Octos?

Octos is a **Rust-native, API-first Agentic OS** for robotics. It turns any LLM into a multi-tenant, safety-enforced robot controller. Think of it as Android for robot brains — one framework that runs on any hardware, any LLM, and enforces industrial safety at every layer.

The name comes from the octopus: 9 brains (1 central + 8 in each arm). Every arm thinks independently, but they share one brain. In octos, every robot runs its own edge agent, but they coordinate through a shared pipeline and safety architecture.

---

## System Architecture

```
                    ┌──────────────────────────────────┐
                    │        USER INTERFACES            │
                    │  CLI  │  Web (91 APIs)  │  14 Ch  │
                    └───────────────┬──────────────────┘
                                    │
                    ┌───────────────▼──────────────────┐
                    │           OCTOS-CLI               │
                    │  Commands, config hot-reload,     │
                    │  auth (OAuth PKCE + device code)  │
                    └───────────────┬──────────────────┘
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
┌───────▼───────┐  ┌────────────────▼───────────────┐  ┌───────▼───────┐
│  OCTOS-BUS    │  │        OCTOS-AGENT             │  │OCTOS-PIPELINE │
│ 14 channels   │  │  Agent loop (realtime)         │  │ DOT-graph DAG │
│ Sessions      │  │  Tool registry (25+ builtins)  │  │ 11 handler    │
│ Coalescing    │  │  Sandbox (bwrap/macOS/Docker)   │  │  kinds        │
│ Cron service  │  │  MCP client (stdio + HTTP)     │  │ Checkpoints   │
└───────────────┘  │  Plugins + Skills              │  │ Invariants    │
                    │  Safety engine                 │  │ Deadlines     │
                    │  Hooks (9 lifecycle events)    │  └───────────────┘
                    │  Black-box recorder            │
                    └──────┬────────────┬───────────┘
                           │            │
                    ┌──────▼──────┐  ┌──▼────────────┐
                    │ OCTOS-MEMORY│  │   OCTOS-LLM   │
                    │ Episodes    │  │ 4 native       │
                    │ Hybrid      │  │   providers    │
                    │   search    │  │ 8 compatible   │
                    │ BM25+Vector │  │ 3-layer        │
                    └─────────────┘  │   failover     │
                                     └────────────────┘
                                              │
                    ┌─────────────────────────▼────────┐
                    │          OCTOS-PLUGIN             │
                    │  Manifest parsing, discovery,     │
                    │  gating, HardwareLifecycle        │
                    └──────────────────────────────────┘
                                              │
                    ┌─────────────────────────▼────────┐
                    │        OCTOS-DORA-MCP            │
                    │  Dora-RS node ←→ MCP tool bridge │
                    │  JSON config, safety tiers       │
                    └──────────────────────────────────┘
                                              │
                    ┌─────────────────────────▼────────┐
                    │          DORA-RS DATAFLOW         │
                    │  Camera → Vision → Planner → Arm │
                    │  Zero-copy, Apache Arrow, Rust   │
                    └──────────────────────────────────┘
```

### Crate Map (18 crates)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| **octos-core** | Shared types, no dependencies | `Task`, `Message`, `MessageRole`, `TokenUsage`, `SessionKey` |
| **octos-memory** | Episodic + long-term memory | `EpisodeStore` (redb), `MemoryStore`, `HybridSearch` (BM25 + HNSW vector) |
| **octos-llm** | LLM abstraction + failover | `LlmProvider` trait, `RetryProvider`, `ProviderChain`, `AdaptiveRouter` |
| **octos-agent** | Agent loop, tools, sandbox, safety | `Agent`, `ToolRegistry`, `Sandbox`, `McpClient`, safety types |
| **octos-bus** | 14-channel message bus | Sessions (JSONL + LRU), coalescing, cron, heartbeat |
| **octos-cli** | CLI entry point | Commands, config watcher (SHA-256 hot-reload), auth module |
| **octos-pipeline** | DOT-graph orchestration | `PipelineGraph`, `HandlerKind` (11 variants), `PipelineExecutor` |
| **octos-plugin** | Plugin SDK | `PluginManifest`, `HardwareLifecycle`, `LifecycleExecutor`, gating |
| **octos-dora-mcp** | Dora-RS bridge | `DoraToolMapping`, `DoraToolBridge`, `BridgeConfig` |
| **app-skills** (9) | Bundled capabilities | weather, time, news, deep-search, deep-crawl, send-email, etc. |
| **platform-skills** | Platform-specific | voice (TTS) |

---

## The Agent Loop

The core of octos is a ReAct (Reasoning + Acting) loop with real-time timing guarantees:

```
┌─────────────────────────────────────────────────────────────┐
│                    AGENT ITERATION                           │
│                                                             │
│  1. heartbeat.beat()              ← Prove agent is alive    │
│  2. check e-stop                  ← Hardware safety gate    │
│  3. inject sensor context         ← SensorContextInjector   │
│  4. build messages                ← System + history + user │
│  5. call LLM (with timeout)      ← llm_timeout_ms budget   │
│  6. if tool calls:                                          │
│     a. authorize (SafetyTier)     ← Permission check        │
│     b. fire BeforeMotion hook     ← External veto point     │
│     c. execute in sandbox         ← Isolation               │
│     d. fire AfterMotion hook      ← Log result              │
│     e. record to black box        ← JSONL audit trail       │
│     f. append results → goto 5                              │
│  7. if EndTurn or budget exceeded → return result           │
│  8. context compaction if needed  ← Prevent OOM             │
│  9. sleep to min_cycle_ms         ← Prevent CPU spin        │
│                                                             │
│  Total iteration: ≤ iteration_deadline_ms (enforced)        │
└─────────────────────────────────────────────────────────────┘
```

**Key guarantee:** The loop never hangs indefinitely. Every LLM call, tool execution, and hook has a timeout. If the agent stalls, the `Heartbeat` monitor detects it within `heartbeat_timeout_ms` and external systems trigger safe-hold.

---

## The 7 Robot Safety Commits (v0.2.1–v0.2.7)

These commits transform octos from a general-purpose agent framework into an **industrial robotics OS**. Each addresses a specific failure mode that would be catastrophic on real hardware.

### How They Fit Together

```
          User says: "Inspect valve V-101"
                        │
                        ▼
  ┌──── v0.2.5: Mission Pipeline ────────────────────────┐
  │  DOT graph: preflight → navigate → inspect → report  │
  │  Deadlines, invariants, checkpoints at each node      │
  └──────────────────────┬───────────────────────────────┘
                         │ At each node:
                         ▼
  ┌──── v0.2.1: Permission Check ────────────────────────┐
  │  policy.authorize("move_joint", SafetyTier::SafeMotion) │
  │  WorkspaceBounds.contains(x, y, z) → reject if outside  │
  └──────────────────────┬───────────────────────────────┘
                         │ If authorized:
                         ▼
  ┌──── v0.2.2: Hook Validation ─────────────────────────┐
  │  HookEvent::BeforeMotion → external PLC/ROS check    │
  │  RobotPayload carries joint positions, force, velocity│
  │  Hook exit(1) = DENY → motion blocked                │
  └──────────────────────┬───────────────────────────────┘
                         │ If hook allows:
                         ▼
  ┌──── v0.2.7: Dora MCP Bridge ────────────────────────┐
  │  DoraToolBridge forwards to dora-rs motion_planner   │
  │  Safety tier enforced at bridge level too             │
  └──────────────────────┬───────────────────────────────┘
                         │ During execution:
                         ▼
  ┌──── v0.2.3: Realtime Loop ──────────────────────────┐
  │  Heartbeat monitors liveness (stall → safe-hold)    │
  │  SensorContextInjector feeds live state to LLM      │
  │  Timing budgets prevent indefinite hangs             │
  └──────────────────────┬───────────────────────────────┘
                         │ Throughout:
                         ▼
  ┌──── v0.2.6: Black-Box Recorder ─────────────────────┐
  │  Every decision → JSONL log (non-blocking)          │
  │  Every authorization, motion, force reading logged   │
  │  Post-incident replay: "what happened at 3 AM?"      │
  └──────────────────────┬───────────────────────────────┘
                         │ At startup/shutdown:
                         ▼
  ┌──── v0.2.4: Hardware Lifecycle ─────────────────────┐
  │  Preflight → Init → Ready Check → [Operating]      │
  │  → Shutdown / Emergency Shutdown                     │
  │  Per-step timeout, retries, critical flags           │
  └─────────────────────────────────────────────────────┘
```

### Commit-by-Commit Breakdown

#### v0.2.1 — Tiered Permission Model

**Problem:** LLM hallucinates `full_actuation("max_speed")` during a camera-only inspection. 30kg arm slams into pipeline at full speed.

**Solution:** Four-tier safety hierarchy where the *operator*, not the LLM, sets the ceiling.

```
SafetyTier::Observe < SafeMotion < FullActuation < EmergencyOverride
```

| Type | Purpose |
|------|---------|
| `SafetyTier` | Ordered enum — compile-time tool safety declarations |
| `WorkspaceBounds` | Axis-aligned 3D box — spatial motion limits |
| `RobotPermissionPolicy` | Session ceiling + optional workspace. `authorize()` gates every tool call |
| `PermissionDenied` | Error with tool name, required tier, allowed tier |

**Integration point:** `Tool` trait extended with `required_safety_tier()` default method. Every tool in the registry can declare its tier.

#### v0.2.2 — Robot Safety Hook Events

**Problem:** Permission tiers gate which tools are callable, but can't validate the *parameters* (is this velocity safe? Is this force normal?). Need external safety systems (PLC, ROS safety node) to inspect and veto.

**Solution:** 5 robot-specific hook events that carry physical state via `RobotPayload`.

| Hook Event | When | Can Deny? |
|------------|------|-----------|
| `BeforeMotion` | Before any motion command | Yes (exit 1) |
| `AfterMotion` | After motion completes | No |
| `ForceLimit` | Force/torque exceeds threshold | No (triggers safe-hold) |
| `WorkspaceBoundary` | Target outside workspace | Auto-denied |
| `EmergencyStop` | E-stop triggered | No (halts everything) |

**RobotPayload builders:**
- `for_motion(joint_positions, velocity)` — attached to BeforeMotion
- `for_force(force_torque)` — attached to ForceLimit

**Circuit breaker:** Hooks auto-disable after 3 consecutive failures (configurable). A broken safety hook doesn't crash the entire system.

#### v0.2.3 — Real-Time Agent Loop

**Problem:** LLM provider hangs for 60 seconds. Robot is powered, in mid-motion, nobody knows it's frozen. Or: LLM makes decisions without knowing battery is at 5%.

**Solution:** Timing budgets + heartbeat + sensor context injection.

| Type | Purpose |
|------|---------|
| `RealtimeConfig` | 5 timing parameters (iteration, heartbeat, LLM, cycle, e-stop) |
| `Heartbeat` | Atomic counter + stall detection. External monitor polls `state()` |
| `SensorSnapshot` | Timestamped sensor reading (sensor_id + JSON value) |
| `SensorContextInjector` | Ring buffer → formatted text block prepended to every LLM call |

**Key insight:** The LLM doesn't need to *call a tool* to see sensor data. The injector automatically includes it in every prompt. The LLM sees `[battery] {"soc_pct": 15}` and decides to return to dock — without being asked.

#### v0.2.4 — Hardware Lifecycle

**Problem:** Robot powers on servos before homing. Arm doesn't know where it is. First move: crash. Or: operator powers off servos before parking arm. Arm falls under gravity.

**Solution:** Ordered lifecycle phases with per-step timeout, retries, and critical flags.

```
Preflight → Init → Ready Check → [Operating] → Shutdown
                                      │ fault
                                      ▼
                               Emergency Shutdown
```

| Type | Purpose |
|------|---------|
| `HardwareLifecycle` | 5 phases, each a `Vec<LifecycleStep>` |
| `LifecycleStep` | label, command, timeout_secs, retries, critical |
| `LifecycleExecutor` | `run_phase()` with retry, timeout, abort-on-critical |
| `PhaseResult` | success, steps_completed, steps_total, error |

**Design choices:** Non-critical steps (conveyor encoder check) log warnings and continue. Critical steps (servo power) abort the entire phase. Emergency shutdown has 2-second timeouts and zero retries.

#### v0.2.5 — Mission Pipeline with Robot Handlers

**Problem:** "Inspect 10 valves on Level 3" is one big LLM prompt. If it fails at valve 6, you start over. No safety gates between steps. No deadlines.

**Solution:** DOT-graph pipeline engine with robot-specific handler types.

| Handler | Purpose |
|---------|---------|
| `SensorCheck` | Read sensor, evaluate condition (battery > 20%?) |
| `Motion` | Execute robot motion command |
| `Grasp` | Execute grasp/release action |
| `SafetyGate` | Require safety condition before proceeding |
| `WaitForEvent` | Wait for external event (sensor trigger, timer) |
| `Codergen` | LLM-driven reasoning step |
| `Gate` | Conditional branching |
| `Parallel` / `DynamicParallel` | Fan-out concurrent execution |

**Per-node features:**
- `DeadlineAction` (Abort / Skip / EmergencyStop)
- `Invariant` (continuous condition monitoring with violation action)
- `MissionCheckpoint` (save state for resume after failure/power loss)

**DOT format** means missions are version-controlled, diffable, code-reviewable.

#### v0.2.6 — Black-Box Recorder

**Problem:** At 3 AM the robot damages a valve. "What happened?" No record of what the agent decided, what tools it called, what sensor readings it saw.

**Solution:** Async JSONL recorder with bounded channel. Non-blocking — drops entries if filesystem is slow, never stalls the control loop.

```rust
recorder.log("before_motion", json!({
    "tool": "valve_turn", "velocity": 0.15,
    "joint_positions": [0.0, 0.5, -0.3, 1.2, 0.0, 0.0],
}));
```

Each line is a self-contained JSON object. Post-incident: `cat log.jsonl | jq '.event'` shows the full timeline.

#### v0.2.7 — Dora-RS MCP Bridge

**Problem:** Dora-RS camera nodes, motion planners, gripper controllers exist as dataflow nodes. The LLM agent needs MCP tools. Bridging them requires custom glue code per node.

**Solution:** JSON config that maps dora-rs nodes to MCP tools with safety tier enforcement.

```json
{
  "tool_name": "move_to_valve",
  "dora_node_id": "motion_planner",
  "dora_output_id": "move_command",
  "safety_tier": "safe_motion",
  "timeout_secs": 30
}
```

`DoraToolBridge` implements the `Tool` trait. The agent sees `move_to_valve` as a regular tool. The bridge enforces the safety tier before forwarding to dora-rs.

---

## Pros and Cons

### Strengths

| Strength | Detail |
|----------|--------|
| **Safety is architecture, not afterthought** | 4-tier permissions, 9 hook events, workspace bounds, and black-box recording are baked into the type system. Every tool declares its tier at compile time. |
| **Rust — zero GC pauses** | No garbage collector means no unpredictable 50ms pauses during force-sensitive manipulation. Memory-safe without runtime cost. `deny(unsafe_code)` workspace-wide. |
| **Vertically integrated** | Safety flows from tool declaration → permission check → hook validation → sandbox execution → hardware actuation. No integration gaps between layers. |
| **LLM-agnostic** | 4 native + 8 compatible providers. 3-layer failover (retry → chain → adaptive router with hedge racing). Swap models via config, not code. |
| **Dora-RS over ROS 2** | 10-17x faster than ROS 2. Zero-copy with Apache Arrow. No DDS complexity. Python & Rust first-class. |
| **Timing guarantees** | Every LLM call, tool execution, and hook has a timeout. Heartbeat detects stalls. The robot never hangs indefinitely. |
| **Auditable** | BlackBoxRecorder captures every safety decision, authorization, motion command, and sensor reading as JSONL. Complete post-incident replay. |
| **Mission resilience** | Checkpoints, deadlines, invariants. Resume after power loss. Per-node timeout with configurable action (abort/skip/e-stop). |
| **Single binary** | 31MB static binary. No Python dependency hell, no ROS workspace setup, no colcon builds. `cargo install` and run. |

### Limitations

| Limitation | Detail | Mitigation |
|-----------|--------|------------|
| **LLM inference latency** | 1-3 seconds per decision. Not suitable for 100ms reactive control (obstacle avoidance, dynamic balance). | Hybrid architecture: octos handles deliberative control; dora-rs reactive controllers handle sub-100ms loops. |
| **Young project** | v0.2.x — not battle-tested at fleet scale. | Comprehensive test suite. Safety features designed defensively (circuit breakers, non-blocking recorder). |
| **Dora-RS ecosystem smaller than ROS 2** | Fewer existing packages, smaller community. | MCP bridge can wrap any executable. Dora-RS interop with ROS 2 via bridge nodes. Growing ecosystem (BAAI, HuggingFace backing). |
| **No formal verification** | Safety relies on runtime checks (allow/deny), not mathematical proofs. | Defense in depth: 4 layers (permission → hook → workspace bounds → hardware e-stop). Sufficient for ISO 10218-1 collaborative robot applications. |
| **LLM hallucination risk** | The LLM can propose unsafe actions. | Hallucinations are caught by the safety stack (tier check → hook veto → workspace rejection). The LLM cannot bypass runtime enforcement. |
| **GPU dependency for edge** | Running local LLMs requires GPU hardware. | Cloud LLM mode for edge robots without GPUs. Or use smaller models (Llama 8B on consumer GPUs). |

### Architectural Trade-offs

| Decision | Trade-off | Rationale |
|----------|-----------|-----------|
| **Rust over Python** | Steeper learning curve for contributors | Zero GC pauses, memory safety, single binary deployment. Critical for hardware-facing code. |
| **Dora-RS over ROS 2** | Smaller ecosystem | 10x performance, simpler architecture, no DDS. ROS 2's 15 years of tech debt is a liability, not an asset. |
| **4-tier model over ACL** | Less granular than per-tool ACLs | Simpler mental model for operators. Tiers map directly to physical risk levels. Easy to audit: "this session is SafeMotion" tells you everything. |
| **JSONL over database** | No structured queries | Append-only, no schema migration, trivially parseable. For flight data recording, simplicity beats queryability. |
| **DOT graphs over YAML/JSON** | Less familiar to developers | Visual (renders in any Graphviz viewer), diffable, version-controllable. Established in pipeline/workflow tooling. |

---

## Deployment Patterns

### Pattern 1: Single Robot (Development)

```
┌─────────────────────────────────┐
│  Laptop / Edge Device           │
│  ┌───────────┐  ┌────────────┐ │
│  │   Octos   │──│  Dora-RS   │ │
│  │   Agent   │  │  Dataflow  │ │
│  └───────────┘  └────────────┘ │
│        ▲                       │
│        │ Cloud LLM API         │
└────────┼───────────────────────┘
         │
    ┌────▼────┐
    │ Claude  │
    │ / GPT   │
    └─────────┘
```

### Pattern 2: Fleet (Production)

```
┌──────────────────────────────────┐
│         CLOUD BRAIN              │
│  Mission planning, fleet mgmt   │
│  Octos Pipeline (DOT DAGs)      │
│  LLM inference (vLLM cluster)   │
└───────────────┬──────────────────┘
                │ Task dispatch
     ┌──────────┼──────────┐
     ▼          ▼          ▼
┌─────────┐ ┌─────────┐ ┌─────────┐
│ Robot 1 │ │ Robot 2 │ │ Robot N │
│ Edge    │ │ Edge    │ │ Edge    │
│ Agent   │ │ Agent   │ │ Agent   │
│ Dora-RS │ │ Dora-RS │ │ Dora-RS │
│ Safety  │ │ Safety  │ │ Safety  │
└─────────┘ └─────────┘ └─────────┘
```

### Pattern 3: Hybrid (Recommended)

```
┌──────────────────────────────────────────────┐
│                EDGE DEVICE                    │
│  ┌──────────────────────────────────────────┐│
│  │ DORA-RS (reactive, <10ms)               ││
│  │ Obstacle avoidance, force control,      ││
│  │ balance, collision detection             ││
│  └────────────────────┬─────────────────────┘│
│                       │ sensor data + events  │
│  ┌────────────────────▼─────────────────────┐│
│  │ OCTOS AGENT (deliberative, 1-3s)        ││
│  │ Task planning, tool selection,           ││
│  │ error recovery, human interaction        ││
│  │ Safety: tiers, hooks, bounds, recording  ││
│  └──────────────────────────────────────────┘│
└──────────────────────────────────────────────┘
```

---

## What This Architecture Enables

| Capability | Without Octos | With Octos |
|-----------|--------------|------------|
| **New inspection task** | Months of programming | Natural language + DOT pipeline |
| **Error recovery** | Manual operator intervention | LLM reasons about failure, retries |
| **Safety audit** | Scattered logs, no replay | JSONL black box, complete timeline |
| **New hardware** | Custom ROS 2 nodes + integration | JSON config in MCP bridge |
| **Multi-robot coordination** | Custom middleware | Pipeline fan-out + fleet dispatch |
| **LLM provider outage** | System down | 3-layer failover, automatic switching |
| **Agent stall** | Robot freezes, undetected | Heartbeat → safe-hold in 5 seconds |
| **Operator shift change** | Reprogram safety params | Change session tier: `SafeMotion` → `Observe` |
