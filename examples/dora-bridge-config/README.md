# Dora MCP Bridge + Mission Pipeline — Config Example

Covers: **Dora MCP Bridge (Area 2)** + **Mission Pipeline (Area 5)**

This directory contains configuration files for wiring a Dora-RS dataflow graph
to octos via the MCP tool bridge, and defining an inspection mission as a DOT
pipeline.

## Files

| File | Purpose |
|------|---------|
| `dora_tool_map.json` | Maps Dora-RS nodes/outputs to MCP-compatible tools |
| `inspection_mission.dot` | DOT pipeline for a valve inspection mission |

## dora_tool_map.json

Defines 4 tools bridged from Dora-RS nodes:

| Tool | Dora Node | Safety Tier |
|------|-----------|-------------|
| `capture_valve_image` | `camera_node` | `observe` |
| `move_to_valve` | `motion_planner` | `safe_motion` |
| `turn_valve` | `gripper_node` | `full_actuation` |
| `read_pressure_gauge` | `vision_node` | `observe` |

Load with `BridgeConfig`:

```rust
use octos_dora_mcp::BridgeConfig;

let config = BridgeConfig::from_file("examples/dora-bridge-config/dora_tool_map.json")?;
for mapping in &config.mappings {
    println!("{} -> {}:{}", mapping.tool_name, mapping.dora_node_id, mapping.dora_output_id);
}
```

## inspection_mission.dot

A pipeline DAG using robot-specific `HandlerKind` variants:

- **SensorCheck** — Read sensor data and evaluate a condition
- **Motion** — Execute a robot motion command
- **SafetyGate** — Require safety condition before proceeding
- **Codergen** — LLM-driven agent step
- **Gate** — Conditional branching

Pipeline attributes demonstrate:

- `deadline_secs` / `deadline_action` — `DeadlineAction::Abort` or `Skip`
- `invariant` / `on_violation` — `Invariant` with `EmergencyStop` action
- `checkpoint="true"` — `MissionCheckpoint` for resumable execution

## Wiring the Bridge

```rust
use octos_dora_mcp::{BridgeConfig, DoraToolBridge};

let config = BridgeConfig::from_file("dora_tool_map.json")?;
let tools: Vec<DoraToolBridge> = config
    .mappings
    .into_iter()
    .map(DoraToolBridge::new)
    .collect();

// Register tools with the agent's ToolRegistry
for tool in &tools {
    println!("Registered: {} (tier: {:?})", tool.mapping().tool_name, tool.required_safety_tier());
}
```
