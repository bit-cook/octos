//! Gas pipeline valve inspection robot — safety demo.
//!
//! Covers: Permissions (Area 1) + Hooks (Area 3) + Recorder (Area 6)
//!
//! ```bash
//! cargo run --example inspection_safety -p octos-agent
//! ```

use std::path::PathBuf;

use octos_agent::{
    BlackBoxRecorder, HookEvent, RobotPayload,
    RobotPermissionPolicy, SafetyTier, WorkspaceBounds,
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // ── 1. Define workspace bounds for a valve inspection cell ──
    let workspace = WorkspaceBounds {
        x_min: -0.5,
        x_max: 0.5,
        y_min: -0.3,
        y_max: 0.3,
        z_min: 0.0,
        z_max: 0.8,
    };

    // ── 2. Create a permission policy (SafeMotion tier with bounds) ──
    let policy = RobotPermissionPolicy::new(SafetyTier::SafeMotion)
        .with_workspace(workspace.clone());

    println!("Policy max tier: {}", policy.max_tier);

    // ── 3. Authorize tools at different tiers ──
    // Camera observe — allowed (Observe <= SafeMotion)
    match policy.authorize("camera_capture", SafetyTier::Observe) {
        Ok(()) => println!("[OK] camera_capture authorized at Observe tier"),
        Err(e) => println!("[DENIED] {e}"),
    }

    // Slow valve turn — allowed (SafeMotion <= SafeMotion)
    match policy.authorize("valve_slow_turn", SafetyTier::SafeMotion) {
        Ok(()) => println!("[OK] valve_slow_turn authorized at SafeMotion tier"),
        Err(e) => println!("[DENIED] {e}"),
    }

    // Full actuation — denied (FullActuation > SafeMotion)
    match policy.authorize("joint_full_actuation", SafetyTier::FullActuation) {
        Ok(()) => println!("[OK] joint_full_actuation authorized"),
        Err(e) => println!("[DENIED] {e}"),
    }

    // ── 4. Validate workspace bounds ──
    let test_points = [
        (0.0, 0.0, 0.4, "center of workspace"),
        (0.6, 0.0, 0.4, "outside X bounds"),
        (0.0, 0.0, -0.1, "below Z floor"),
    ];

    println!("\nWorkspace bounds check:");
    for (x, y, z, label) in &test_points {
        let inside = workspace.contains(*x, *y, *z);
        let icon = if inside { "INSIDE" } else { "OUTSIDE" };
        println!("  ({x:.1}, {y:.1}, {z:.1}) {label} -> [{icon}]");
    }

    // ── 5. Construct a RobotPayload for a motion event ──
    let robot_payload = RobotPayload::for_motion(
        vec![0.0, 0.5, -0.3, 1.2, 0.0, 0.0],
        Some(0.15),
    );
    println!(
        "\nRobotPayload: {} joints, velocity={:?}",
        robot_payload.joint_positions.len(),
        robot_payload.velocity
    );

    // ── 6. Demonstrate hook events for robot safety ──
    let hook_events = [
        HookEvent::BeforeMotion,
        HookEvent::AfterMotion,
        HookEvent::ForceLimit,
        HookEvent::WorkspaceBoundary,
        HookEvent::EmergencyStop,
    ];
    println!("\nRobot safety hook events:");
    for event in &hook_events {
        println!("  {event:?}");
    }

    // ── 7. Record events to a JSONL black box ──
    let log_path = PathBuf::from("/tmp/inspection_safety_demo.jsonl");
    let recorder = BlackBoxRecorder::new(log_path.clone(), 256).await?;

    recorder.log(
        "inspection_start",
        serde_json::json!({
            "policy_tier": policy.max_tier.label(),
            "workspace_x": [workspace.x_min, workspace.x_max],
        }),
    );

    recorder.log(
        "authorization",
        serde_json::json!({
            "tool": "camera_capture",
            "tier": "observe",
            "result": "allowed",
        }),
    );

    recorder.log(
        "authorization",
        serde_json::json!({
            "tool": "joint_full_actuation",
            "tier": "full_actuation",
            "result": "denied",
        }),
    );

    recorder.log(
        "before_motion",
        serde_json::json!({
            "tool": "valve_slow_turn",
            "joint_positions": [0.0, 0.5, -0.3, 1.2, 0.0, 0.0],
            "velocity": 0.15,
        }),
    );

    // Force-limit payload
    let force_payload = RobotPayload::for_force(vec![1.2, 0.3, 15.8, 0.0, 0.0, 0.0]);
    recorder.log(
        "force_limit",
        serde_json::json!({
            "force_torque": force_payload.force_torque,
        }),
    );

    assert!(recorder.is_active(), "recorder should be active");

    // Drop to flush remaining entries.
    drop(recorder);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    println!("\nBlack-box log written to {}", log_path.display());
    println!("Inspection safety demo complete.");
    Ok(())
}
