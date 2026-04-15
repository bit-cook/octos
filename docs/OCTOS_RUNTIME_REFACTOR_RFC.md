# Octos Runtime Refactor RFC

## Purpose

This RFC turns the runtime refactor into a bounded, feature-driven program of work.

It is based on a clean review of current `main`, not on the checkpointed local refactor branch.
The goal is to improve long free-form work, delegated background work, and deliverable-heavy
workflows without regressing the parts of Octos that already behave correctly.

This RFC does **not** propose turning all chat into a workflow engine.

## Current-State Findings

### 1. The loop runtime is still an inline iterative agent loop

Current behavior is centered in:

- `crates/octos-agent/src/agent/loop_runner.rs`
- `crates/octos-agent/src/agent/execution.rs`

`process_message_inner()` still owns iteration counting, budget checking, context trimming,
tool-result truncation, and repair steps inline. There is no durable or typed `TurnState`
object. This makes long-turn behavior hard to reason about and easy to regress.

### 2. Compaction and tool-result repair are scattered, not a subsystem

The loop currently performs message normalization and truncation in-place:

- `trim_to_context_window`
- `normalize_system_messages`
- `repair_message_order`
- `repair_tool_pairs`
- `synthesize_missing_tool_results`
- `truncate_old_tool_results`
- `normalize_tool_call_ids`

These are correctness-sensitive operations, but they are not isolated behind one policy or
state boundary.

### 3. Prompt-driven "workflow detection" is still pretending to be runtime control

Current forced-background logic lives in:

- `crates/octos-cli/src/session_actor.rs`

`ForcedBackgroundWorkflow::{DeepResearch, ResearchPodcast}` is still a keyword detector plus
prompt instructions and tool allowlists. That is useful as a stopgap, but it is not a real
workflow runtime. The model still acts as the workflow scheduler.

### 4. Deliverable completion semantics are still split

Spawn-only completion currently mixes several mechanisms:

- contract-backed artifact verification in `workspace_contract.rs`
- fallback `send_file` delivery in `execution.rs`
- tool-local `files_to_send` / `files_modified` reporting

This is a major side-effect risk. It is the root shape behind duplicate delivery, report/file
pollution, and "task complete but result semantics are ambiguous" behavior.

### 5. Contract evaluation is still split into separate engines

Project/workspace checks and spawn-task checks do not fully share the same action runtime.
That means new validators can silently diverge between "inspect the workspace" and "enforce
runtime completion truth".

### 6. The web client still derives truth from mixed sources

Current web behavior spans:

- committed session events
- `/tasks` polling
- `/messages` polling
- `/files` hydration
- status flags like `active`, `has_bg_tasks`, `has_deferred_files`
- fuzzy optimistic/history reconciliation in `message-store.ts`

This is better than before, but correctness is still spread across multiple partial signals.

### 7. Task state remains non-durable

`TaskSupervisor` is still in-memory. That means restart semantics for long work are still
weaker than the rest of the session-result path.

## Design Goals

1. Keep free-form chat and coding work free-form.
2. Add a stronger loop governor for long turns.
3. Turn delegated/background runs into real child sessions, not ad hoc spawned tasks.
4. Add a workflow runtime only for deliverable-heavy task families.
5. Make the session result ledger the only user-visible completion authority.
6. Keep transport and UI projection as projections of committed result truth, not as
   independent correctness layers.

## Non-Goals

- Do not require users to write workflow DSL.
- Do not force all work into explicit graphs.
- Do not let the model invent arbitrary workflow topology.
- Do not rewrite the whole web client at once.
- Do not collapse loop-governor work and workflow work into the same patch lane.

## Target Runtime Layers

### 1. Loop Governor

Responsible for:

- typed turn state
- turn budgets
- tool-result budget/compaction
- repair/normalization policy
- concurrent vs serial tool execution policy
- idle/activity-aware timeout integration

Primary code surface:

- `crates/octos-agent/src/agent/loop_runner.rs`
- `crates/octos-agent/src/agent/execution.rs`
- new supporting modules under `crates/octos-agent/src/agent/`

### 2. Child Session Runtime

Responsible for:

- parent/child linkage for delegated runs
- child lifecycle and terminal status
- persisted child results
- stable parent notification semantics
- replacing "spawned task" as the only abstraction for long delegated work

Primary code surface:

- `crates/octos-agent/src/tools/spawn.rs`
- `crates/octos-agent/src/task_supervisor.rs`
- `crates/octos-cli/src/session_actor.rs`
- `crates/octos-bus/src/session.rs`

### 3. Contract Engine v2

Responsible for:

- one shared action engine for workspace checks and spawn/task checks
- richer validators
- separating artifact verification from user-visible delivery
- making "success" mean "verified outputs exist", not "some tool tried to send a file"

Primary code surface:

- `crates/octos-agent/src/workspace_contract.rs`
- `crates/octos-agent/src/workspace_policy.rs`
- `crates/octos-agent/src/behaviour.rs`
- `crates/octos-agent/src/workspace_git.rs`

### 4. Workflow Runtime

Responsible for:

- typed workflow families
- explicit phases
- per-phase tool policy
- per-phase retry policy
- workflow-local scratch state
- final artifact expectations

Primary code surface:

- `crates/octos-cli/src/session_actor.rs`
- new workflow modules in `crates/octos-agent` or `crates/octos-cli` as appropriate
- contract integration via Contract Engine v2

### 5. Session Result Ledger and Web Projection

Responsible for:

- ordered committed session events
- one authoritative path for background completion
- transport as delivery, not as correctness
- web stores as projections of committed events

Primary code surface:

- `crates/octos-bus/src/session.rs`
- `crates/octos-bus/src/api_channel.rs`
- `crates/octos-cli/src/session_actor.rs`
- `octos-web/src/runtime/*`
- `octos-web/src/store/*`

## Workstream Map

### #393 Loop governor state machine for long free-form turns

Scope:

- introduce a typed `TurnState`
- move iteration, budgeting, and loop-phase bookkeeping out of the ad hoc local variables
- make loop transitions explicit enough for testing

Must touch:

- `crates/octos-agent/src/agent/loop_runner.rs`
- `crates/octos-agent/src/agent/mod.rs`

Should avoid touching:

- workflow-family detection in `session_actor.rs`
- web event/result code

Acceptance tests:

- long tool-heavy turn still preserves message/tool ordering
- loop exits with explicit terminal reason
- no regression in ordinary non-background turns

### #394 Tool-result budget and compaction for long free-form turns

Scope:

- extract truncation/repair/normalization into a dedicated subsystem
- make compaction policy testable outside the full agent loop

Must touch:

- `crates/octos-agent/src/agent/loop_runner.rs`
- new compaction/budget helper module(s)

Should avoid touching:

- session result delivery
- workflow family code

Acceptance tests:

- oversized tool outputs are budgeted deterministically
- repaired tool history stays valid after compaction
- missing/partial tool-result chains do not poison later turns

### #395 Promote delegated runs into a real child-session runtime

Scope:

- make spawned runs first-class child sessions
- persist parent/child linkage and terminal outcome semantics
- stop relying on in-memory-only task state for correctness

Must touch:

- `crates/octos-agent/src/tools/spawn.rs`
- `crates/octos-agent/src/task_supervisor.rs`
- `crates/octos-cli/src/session_actor.rs`
- `crates/octos-bus/src/session.rs`

Depends on:

- `#384` for durable task state

Should avoid touching:

- contract validators
- workflow family logic beyond the child-session boundary

Acceptance tests:

- child session survives parent reconnect
- parent receives exactly one terminal child outcome
- restart does not erase in-flight child state

### #396 Contract engine v2: shared action engine plus richer validators

Scope:

- unify the action engine used by workspace inspection and spawn/task enforcement
- add richer validators only after the shared engine exists
- remove contract-owned user delivery semantics from the success boundary

Must touch:

- `crates/octos-agent/src/workspace_contract.rs`
- `crates/octos-agent/src/behaviour.rs`
- `crates/octos-agent/src/workspace_git.rs`
- `crates/octos-agent/src/workspace_policy.rs`

Should avoid touching:

- session/web projection semantics
- prompt-detected workflow routing

Acceptance tests:

- same validator behaves identically in workspace inspection and runtime enforcement
- verified artifact resolution does not double-send files
- session contracts can require multiple outputs where needed

### #397 Workflow runtime core for deliverable-heavy task families

Scope:

- add `WorkflowKind`, `WorkflowInstance`, `WorkflowPhase`, `WorkflowLimits`
- compile detected deliverable-heavy intents into runtime-owned workflow instances
- replace prompt-only workflow steering with typed runtime state

Must touch:

- `crates/octos-cli/src/session_actor.rs`
- new workflow runtime module(s)

Depends on:

- `#395`
- `#396`

Should avoid touching:

- low-level loop compaction internals
- web store reconciliation

Acceptance tests:

- workflow instance persists phase changes
- workflow phase owns tool budget and terminal success/failure conditions
- failure in one phase does not leak partial deliverables as success

### #398 Workflow family: bounded `research_report` runtime

Scope:

- implement a real `research_report` workflow family
- bound search depth, scratch output, and final report semantics

Must touch:

- workflow runtime core
- deep research integration points

Should avoid touching:

- podcast-specific generation semantics

Acceptance tests:

- exactly one report success outcome or one durable failure
- intermediate scratch artifacts are not delivered as final outputs

### #399 Workflow family: runtime-owned `research_podcast` with one final deliverable

Scope:

- replace prompt-driven `ResearchPodcast` routing with a real workflow
- explicit phases: research, script, audio generation, delivery
- exactly one final MP3 or exactly one durable failure

Must touch:

- `crates/octos-cli/src/session_actor.rs`
- workflow runtime core
- contract engine v2
- relevant podcast/tool integration points

Should avoid touching:

- general site/slides workflows

Acceptance tests:

- no duplicate MP3 delivery
- no `_report.md` or intermediate research output delivered as final media
- `podcast_generate` failure cannot fall through into partial TTS success paths

### #400 Workflow families for slides and site deliverables

Scope:

- make site/slides deliverable truth explicit and template-aware
- avoid one static site artifact assumption across templates

Must touch:

- `crates/octos-agent/src/workspace_policy.rs`
- project template metadata integration
- workflow runtime core if site/slides are routed through it

Should avoid touching:

- research-specific workflow budgets

Acceptance tests:

- per-template site output validation
- slide generation returns one verified deck outcome, not mixed scratch files

### #401 Live browser long-task smoke suite for runtime refactor acceptance

Scope:

- convert known regressions into live browser smoke coverage
- cover ordering, duplicate media, reload durability, and final artifact visibility

Must touch:

- `octos-web/tests/*`
- any supporting test helpers

Should avoid touching:

- production runtime unless a testability hook is required

Acceptance tests:

- short TTS success renders exactly one audio attachment
- long research reload does not synthesize bogus turns
- research podcast produces exactly one final audio attachment after reload

### #402 Activity-based timeout policy for long-running active turns

Scope:

- distinguish idle wedged work from actively progressing work
- apply this to long loop turns and child sessions

Must touch:

- loop governor
- child-session runtime

Should avoid touching:

- workflow-family specifics beyond timeout hooks

Acceptance tests:

- active long turn does not timeout simply on wall clock
- idle wedged run still times out and persists a durable failure

## Parallel Lane Boundaries

Safe concurrent lanes:

1. `#393` + `#402`
2. `#394`
3. `#395`
4. `#396`
5. `#384` + `#385`
6. `#389` + `#390` + `#391`
7. `#397`
8. `#398`
9. `#399`
10. `#400`
11. `#401`

Reasoning:

- loop control should not share a lane with workflow-family implementation
- web/event projection should not share a lane with contract-engine refactors
- workflow core should land before workflow families
- smoke tests need to target merged runtime behavior, not provisional behavior

## Merge Order

1. `#393`, `#394`, `#402`
2. `#384`, `#385`
3. `#395`
4. `#396`
5. `#389`, `#390`, `#391`
6. `#397`
7. `#398`, `#399`, `#400`
8. `#401`

## The Main Rule

For deliverable-heavy work, the model should not decide what "done" means.

The runtime should own:

- phase transitions
- tool budgets
- artifact verification
- terminal success/failure semantics
- durable session delivery
