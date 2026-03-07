//! Voice transcription via ASR platform skill binary.

use std::time::Duration;

use eyre::{Result, WrapErr};
use tokio::io::AsyncWriteExt;

/// Transcribe audio by spawning the asr platform skill binary.
pub async fn transcribe_via_skill(
    asr_binary: &std::path::Path,
    input_json: &str,
) -> Result<String> {
    let mut child = tokio::process::Command::new(asr_binary)
        .arg("voice_transcribe")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .wrap_err("failed to spawn asr skill binary")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input_json.as_bytes()).await?;
        drop(stdin);
    }

    let output = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
        .await
        .map_err(|_| eyre::eyre!("asr transcription timed out"))??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value =
        serde_json::from_str(&stdout).wrap_err("invalid asr skill output")?;

    if result.get("success").and_then(|v| v.as_bool()) == Some(true) {
        Ok(result["output"].as_str().unwrap_or("").to_string())
    } else {
        let msg = result["output"].as_str().unwrap_or("unknown error");
        eyre::bail!("asr skill failed: {msg}")
    }
}
