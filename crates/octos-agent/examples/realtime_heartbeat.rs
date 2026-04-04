//! 24/7 patrol robot monitoring — realtime loop demo.
//!
//! Covers: Realtime Loop (Area 8) + Sensor Injection
//!
//! ```bash
//! cargo run --example realtime_heartbeat -p octos-agent
//! ```

use std::time::Duration;

use octos_agent::{
    Heartbeat, HeartbeatState, RealtimeConfig, SensorContextInjector, SensorSnapshot,
};

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[tokio::main]
async fn main() {
    // ── 1. Configure the real-time loop ──
    let config = RealtimeConfig {
        iteration_deadline_ms: 3000,
        heartbeat_timeout_ms: 5000,
        llm_timeout_ms: 4000,
        min_cycle_ms: 200,
        check_estop: true,
    };
    println!("RealtimeConfig:");
    println!("  iteration_deadline: {}ms", config.iteration_deadline_ms);
    println!("  heartbeat_timeout:  {}ms", config.heartbeat_timeout_ms);
    println!("  llm_timeout:        {}ms", config.llm_timeout_ms);
    println!("  min_cycle:          {}ms", config.min_cycle_ms);
    println!("  check_estop:        {}", config.check_estop);

    // ── 2. Heartbeat: beat/check cycle ──
    let heartbeat = Heartbeat::new(Duration::from_millis(config.heartbeat_timeout_ms));

    // Simulate 5 agent loop iterations
    println!("\nHeartbeat simulation (5 beats):");
    for i in 1..=5 {
        heartbeat.beat();
        let state = heartbeat.state();
        println!("  beat {i}: count={}, state={state:?}", heartbeat.count());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Simulate stall by not beating and waiting past timeout
    println!("\nSimulating stall (waiting {}ms with no beats)...", config.heartbeat_timeout_ms);
    // Consume current check value
    let _ = heartbeat.state();
    tokio::time::sleep(Duration::from_millis(config.heartbeat_timeout_ms + 500)).await;
    let stall_state = heartbeat.state();
    println!("  state after timeout: {stall_state:?}");
    assert_eq!(stall_state, HeartbeatState::Stalled);

    // Recovery: beat again
    heartbeat.beat();
    let recovered = heartbeat.state();
    println!("  state after recovery beat: {recovered:?}");
    assert_eq!(recovered, HeartbeatState::Alive);

    // ── 3. Sensor context injection ──
    let mut injector = SensorContextInjector::new(8);

    // Push patrol robot sensor readings
    let sensors = [
        ("lidar_front", serde_json::json!({"range_m": 3.2, "clear": true})),
        ("battery", serde_json::json!({"voltage": 24.1, "soc_pct": 78})),
        ("joint_positions", serde_json::json!([0.0, 0.5, -0.3, 1.2, 0.0, 0.0])),
        ("imu", serde_json::json!({"roll": 0.01, "pitch": -0.02, "yaw": 1.57})),
        ("force_torque", serde_json::json!([0.5, 0.1, 9.8, 0.0, 0.0, 0.0])),
    ];

    println!("\nInjecting {} sensor snapshots:", sensors.len());
    for (id, value) in sensors {
        let snapshot = SensorSnapshot {
            sensor_id: id.to_string(),
            value,
            timestamp_ms: now_ms(),
        };
        println!("  {}", snapshot.to_context_line());
        injector.push(snapshot);
    }

    println!("\nSensor buffer: {} / 8 slots used", injector.len());

    // Retrieve latest reading for a specific sensor
    if let Some(latest) = injector.latest("battery") {
        println!("Latest battery: {}", latest.value);
    }

    // Format the full context block for LLM injection
    let context_block = injector.to_context_block();
    println!("\n--- LLM Context Block ---");
    println!("{context_block}");
    println!("--- End Context Block ---");

    println!("\nRealtime heartbeat demo complete.");
}
