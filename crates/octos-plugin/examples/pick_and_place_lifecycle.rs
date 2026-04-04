//! Pick-and-place workcell startup/shutdown — lifecycle demo.
//!
//! Covers: Hardware Lifecycle (Area 4)
//!
//! ```bash
//! cargo run --example pick_and_place_lifecycle -p octos-plugin
//! ```

use octos_plugin::{HardwareLifecycle, LifecycleExecutor, LifecycleStep};

fn step(label: &str, command: &str, timeout: u64, retries: u32, critical: bool) -> LifecycleStep {
    LifecycleStep {
        label: label.to_string(),
        command: command.to_string(),
        timeout_secs: timeout,
        retries,
        critical,
    }
}

#[tokio::main]
async fn main() {
    // ── 1. Define the hardware lifecycle for a pick-and-place workcell ──
    let lifecycle = HardwareLifecycle {
        preflight: vec![
            step("Check gripper air supply", "echo 'Air pressure: 6.2 bar OK'", 5, 1, true),
            step("Check camera connection", "echo 'Camera /dev/video0 ready'", 5, 0, true),
            step("Check conveyor encoder", "echo 'Encoder pulses: nominal'", 5, 0, false),
        ],
        init: vec![
            step("Power on servo drives", "echo 'Servo drives powered'", 10, 2, true),
            step("Home all axes", "echo 'Homing complete: 6 axes at zero'", 30, 1, true),
            step("Open gripper", "echo 'Gripper opened'", 5, 0, true),
            step("Start conveyor", "echo 'Conveyor running at 0.2 m/s'", 5, 0, false),
        ],
        ready_check: vec![
            step("Verify joint limits", "echo 'All joints within limits'", 5, 0, true),
            step("Verify force sensor zero", "echo 'Force sensor zeroed: [0.01, 0.00, 0.02]'", 5, 0, true),
            step("Verify workspace clear", "echo 'Workspace clear — no obstacles detected'", 5, 0, true),
        ],
        shutdown: vec![
            step("Park arm at home", "echo 'Arm parked at home position'", 15, 1, true),
            step("Open gripper", "echo 'Gripper released'", 5, 0, true),
            step("Stop conveyor", "echo 'Conveyor stopped'", 5, 0, false),
            step("Power off servo drives", "echo 'Servos powered off'", 10, 0, true),
        ],
        emergency_shutdown: vec![
            step("Emergency stop all axes", "echo 'E-STOP: all axes halted'", 2, 0, true),
            step("Vent gripper pressure", "echo 'Gripper pressure vented'", 2, 0, true),
        ],
    };

    // ── 2. Run each phase with the LifecycleExecutor ──
    let phases: Vec<(&str, &[LifecycleStep])> = vec![
        ("preflight", &lifecycle.preflight),
        ("init", &lifecycle.init),
        ("ready_check", &lifecycle.ready_check),
    ];

    for (name, steps) in &phases {
        println!("=== Phase: {name} ({} steps) ===", steps.len());
        let result = LifecycleExecutor::run_phase(name, steps).await;
        println!(
            "  Result: {} ({}/{} steps completed)\n",
            if result.success { "SUCCESS" } else { "FAILED" },
            result.steps_completed,
            result.steps_total,
        );
        if let Some(err) = &result.error {
            println!("  Error: {err}");
        }
    }

    // ── 3. Simulate graceful shutdown ──
    println!("=== Phase: shutdown ({} steps) ===", lifecycle.shutdown.len());
    let shutdown_result = LifecycleExecutor::run_phase("shutdown", &lifecycle.shutdown).await;
    println!(
        "  Result: {} ({}/{} steps completed)\n",
        if shutdown_result.success { "SUCCESS" } else { "FAILED" },
        shutdown_result.steps_completed,
        shutdown_result.steps_total,
    );

    // ── 4. Show emergency shutdown path ──
    println!(
        "=== Phase: emergency_shutdown ({} steps) ===",
        lifecycle.emergency_shutdown.len()
    );
    let estop_result =
        LifecycleExecutor::run_phase("emergency_shutdown", &lifecycle.emergency_shutdown).await;
    println!(
        "  Result: {} ({}/{} steps completed)",
        if estop_result.success { "SUCCESS" } else { "FAILED" },
        estop_result.steps_completed,
        estop_result.steps_total,
    );

    println!("\nPick-and-place lifecycle demo complete.");
}
