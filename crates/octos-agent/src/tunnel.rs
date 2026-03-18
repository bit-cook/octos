//! Tunnel support for exposing local HTTP servers to the internet.
//!
//! Currently supports Cloudflare Quick Tunnels (no account required).
//! Future: ngrok, frp, custom relay.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use eyre::{bail, eyre, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Supported tunnel providers
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TunnelProvider {
    /// Cloudflare Quick Tunnel (free, random domain)
    #[serde(rename = "cloudflare")]
    Cloudflare,
    /// Cloudflare Named Tunnel (custom domain like xxx.mofa.ai)
    #[serde(rename = "cloudflare-named")]
    CloudflareNamed,
    /// ngrok (requires auth token)
    #[serde(rename = "ngrok")]
    Ngrok,
    /// Auto-detect based on availability
    #[serde(rename = "auto")]
    Auto,
}

impl Default for TunnelProvider {
    fn default() -> Self {
        TunnelProvider::Cloudflare
    }
}

impl TunnelProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TunnelProvider::Cloudflare => "cloudflare",
            TunnelProvider::CloudflareNamed => "cloudflare-named",
            TunnelProvider::Ngrok => "ngrok",
            TunnelProvider::Auto => "auto",
        }
    }
}

/// An active tunnel connection
#[derive(Debug)]
pub struct Tunnel {
    pub public_url: String,
    pub local_port: u16,
    pub provider: TunnelProvider,
    pub(crate) process: Child,
}

impl Tunnel {
    /// Start a new tunnel to expose a local port
    ///
    /// # Arguments
    /// * `local_port` - The local port to expose
    /// * `provider` - Which tunnel provider to use
    pub async fn start(local_port: u16, provider: TunnelProvider) -> Result<Self> {
        match provider {
            TunnelProvider::Cloudflare => Self::start_cloudflare(local_port).await,
            TunnelProvider::CloudflareNamed => {
                bail!("CloudflareNamed provider requires tunnel_name, use start_named() instead")
            }
            TunnelProvider::Ngrok => Self::start_ngrok(local_port).await,
            TunnelProvider::Auto => {
                // Try cloudflare first, then ngrok
                if let Ok(tunnel) = Self::start_cloudflare(local_port).await {
                    Ok(tunnel)
                } else {
                    Self::start_ngrok(local_port).await
                }
            }
        }
    }

    /// Start a named Cloudflare tunnel with custom domain
    ///
    /// # Arguments
    /// * `local_port` - The local port to expose
    /// * `tunnel_name` - Name of the pre-created tunnel (e.g., "crew-agent-001")
    /// * `subdomain` - The subdomain to use (e.g., "xxx" for xxx.mofa.ai)
    /// * `domain` - The root domain (e.g., "mofa.ai")
    pub async fn start_named(
        local_port: u16,
        tunnel_name: &str,
        subdomain: &str,
        domain: &str,
    ) -> Result<Self> {
        let cf_path = Self::ensure_cloudflared().await?;
        let public_url = format!("https://{}.{}", subdomain, domain);

        info!(
            port = local_port,
            tunnel = %tunnel_name,
            url = %public_url,
            "Starting Cloudflare named tunnel"
        );

        // Create a temporary config file for this tunnel instance
        let config_dir = dirs::home_dir()
            .ok_or_else(|| eyre!("Cannot determine home directory"))?
            .join(".crew/cloudflared-configs");
        tokio::fs::create_dir_all(&config_dir).await?;

        let config_path = config_dir.join(format!("{}-{}.yml", tunnel_name, local_port));
        let config_content = format!(
            "tunnel: {}\ncredentials-file: {}\ningress:\n  - hostname: {}\n    service: http://localhost:{}\n  - service: http_status:404\n",
            tunnel_name,
            dirs::home_dir()
                .unwrap()
                .join(format!(".cloudflared/{}.json", tunnel_name))
                .display(),
            public_url,
            local_port
        );
        tokio::fs::write(&config_path, config_content).await?;

        let mut child = Command::new(&cf_path)
            .args(["tunnel", "--config", config_path.to_str().unwrap(), "run"])
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| eyre!("Failed to spawn cloudflared: {}", e))?;

        // For named tunnels, we trust the config is correct
        // but we still wait a moment to ensure it doesn't immediately fail
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| eyre!("Failed to capture stderr"))?;

        match Self::wait_for_tunnel_ready(stderr, &public_url).await {
            Ok(_) => Ok(Self {
                public_url,
                local_port,
                provider: TunnelProvider::CloudflareNamed,
                process: child,
            }),
            Err(e) => {
                let _ = child.kill().await;
                Err(e)
            }
        }
    }

    /// Wait for tunnel to be ready (check stderr for errors)
    async fn wait_for_tunnel_ready(
        stderr: tokio::process::ChildStderr,
        _expected_url: &str,
    ) -> Result<()> {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        let result = timeout(Duration::from_secs(30), async {
            while let Some(line) = lines.next_line().await.transpose() {
                match line {
                    Ok(line) => {
                        debug!("cloudflared: {}", line);

                        // Check for errors
                        if line.contains("error") || line.contains("failed") || line.contains("ERR") {
                            return Err(eyre!("cloudflared error: {}", line));
                        }

                        // Success indicators for named tunnels
                        if line.contains("Connected") || line.contains("registered conn") {
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        return Err(eyre!("Failed to read cloudflared output: {}", e));
                    }
                }
            }
            // If we got here without errors, assume it's working
            Ok(())
        })
        .await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(eyre!("Timeout waiting for tunnel to be ready (30s)")),
        }
    }

    /// Start a Cloudflare Quick Tunnel
    async fn start_cloudflare(local_port: u16) -> Result<Self> {
        let cf_path = Self::ensure_cloudflared().await?;

        info!(
            port = local_port,
            "Starting Cloudflare Quick Tunnel"
        );

        let mut child = Command::new(&cf_path)
            .args([
                "tunnel",
                "--url",
                &format!("http://localhost:{}", local_port),
            ])
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| eyre!("Failed to spawn cloudflared: {}", e))?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| eyre!("Failed to capture stderr"))?;

        // Parse the public URL from logs with timeout
        let public_url = match Self::parse_cloudflare_url(stderr).await {
            Ok(url) => url,
            Err(e) => {
                let _ = child.kill().await;
                return Err(e);
            }
        };

        info!(
            port = local_port,
            url = %public_url,
            "Cloudflare tunnel established"
        );

        Ok(Self {
            public_url,
            local_port,
            provider: TunnelProvider::Cloudflare,
            process: child,
        })
    }

    /// Parse the tunnel URL from cloudflared stderr output
    async fn parse_cloudflare_url(stderr: tokio::process::ChildStderr) -> Result<String> {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        // Regex to match trycloudflare.com URLs
        let url_regex = Regex::new(r"https://[a-z0-9-]+\.trycloudflare\.com").unwrap();

        // Wait up to 30 seconds for the URL
        let result = timeout(Duration::from_secs(30), async {
            while let Some(line) = lines.next_line().await.transpose() {
                match line {
                    Ok(line) => {
                        debug!("cloudflared: {}", line);

                        if let Some(m) = url_regex.find(&line) {
                            return Ok(m.as_str().to_string());
                        }

                        // Check for errors
                        if line.contains("error") || line.contains("failed") {
                            warn!("cloudflared error: {}", line);
                        }
                    }
                    Err(e) => {
                        return Err(eyre!("Failed to read cloudflared output: {}", e));
                    }
                }
            }
            Err(eyre!("cloudflared exited without producing URL"))
        })
        .await;

        match result {
            Ok(Ok(url)) => Ok(url),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(eyre!("Timeout waiting for tunnel URL (30s)")),
        }
    }

    /// Ensure cloudflared binary is available, downloading if necessary
    async fn ensure_cloudflared() -> Result<PathBuf> {
        // First check if cloudflared is in PATH
        if let Ok(output) = Command::new("cloudflared").arg("--version").output().await {
            if output.status.success() {
                debug!("Using system cloudflared");
                return Ok(PathBuf::from("cloudflared"));
            }
        }

        // Check for cached binary in ~/.crew/bin/
        let home = dirs::home_dir().ok_or_else(|| eyre!("Cannot determine home directory"))?;
        let cached = home.join(".crew/bin/cloudflared");

        if cached.exists() {
            debug!("Using cached cloudflared");
            return Ok(cached);
        }

        // Download cloudflared
        info!("Downloading cloudflared...");
        Self::download_cloudflared(&cached).await?;

        Ok(cached)
    }

    /// Download cloudflared binary for current platform
    async fn download_cloudflared(dest: &Path) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        #[cfg(target_os = "macos")]
        let url = if cfg!(target_arch = "aarch64") {
            "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-arm64"
        } else {
            "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-amd64"
        };

        #[cfg(target_os = "linux")]
        let url = if cfg!(target_arch = "aarch64") {
            "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-arm64"
        } else {
            "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64"
        };

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        bail!("Unsupported platform for auto-downloading cloudflared");

        // Download with redirect following
        let response = reqwest::get(url).await?;

        if !response.status().is_success() {
            bail!(
                "Failed to download cloudflared: HTTP {}",
                response.status()
            );
        }

        let bytes = response.bytes().await?;

        // Create directory if needed
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Write and make executable
        let mut file = tokio::fs::File::create(dest).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        drop(file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            tokio::fs::set_permissions(dest, perms).await?;
        }

        info!("Downloaded cloudflared to {:?}", dest);
        Ok(())
    }

    /// Start an ngrok tunnel (requires auth token)
    async fn start_ngrok(local_port: u16) -> Result<Self> {
        // Check if ngrok is available
        match Command::new("ngrok").arg("--version").output().await {
            Ok(output) if output.status.success() => {}
            _ => bail!("ngrok not found. Install from https://ngrok.com"),
        }

        info!(
            port = local_port,
            "Starting ngrok tunnel"
        );

        let mut child = Command::new("ngrok")
            .args(["http", &local_port.to_string(), "--log=stdout"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| eyre!("Failed to spawn ngrok: {}", e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre!("Failed to capture stdout"))?;

        // Parse ngrok URL from logs
        let public_url = match Self::parse_ngrok_url(stdout).await {
            Ok(url) => url,
            Err(e) => {
                let _ = child.kill().await;
                return Err(e);
            }
        };

        info!(
            port = local_port,
            url = %public_url,
            "ngrok tunnel established"
        );

        Ok(Self {
            public_url,
            local_port,
            provider: TunnelProvider::Ngrok,
            process: child,
        })
    }

    /// Parse the tunnel URL from ngrok output
    async fn parse_ngrok_url(stdout: tokio::process::ChildStdout) -> Result<String> {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        // Regex to match ngrok URLs
        let url_regex = Regex::new(r"https://[a-z0-9-]+\.ngrok-free\.app").unwrap();

        let result = timeout(Duration::from_secs(30), async {
            while let Some(line) = lines.next_line().await.transpose() {
                match line {
                    Ok(line) => {
                        debug!("ngrok: {}", line);

                        if let Some(m) = url_regex.find(&line) {
                            return Ok(m.as_str().to_string());
                        }

                        if line.contains("ERR_NGROK") || line.contains("error") {
                            return Err(eyre!("ngrok error: {}", line));
                        }
                    }
                    Err(e) => {
                        return Err(eyre!("Failed to read ngrok output: {}", e));
                    }
                }
            }
            Err(eyre!("ngrok exited without producing URL"))
        })
        .await;

        match result {
            Ok(Ok(url)) => Ok(url),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(eyre!("Timeout waiting for ngrok URL (30s)")),
        }
    }

    /// Get the public URL
    pub fn url(&self) -> &str {
        &self.public_url
    }

    /// Gracefully shutdown the tunnel
    pub async fn stop(mut self) -> Result<()> {
        info!(
            url = %self.public_url,
            "Stopping tunnel"
        );

        let _ = self.process.kill().await;

        // Wait a moment for cleanup
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(())
    }
}

/// Utility function to quickly start a tunnel
pub async fn quick_tunnel(local_port: u16) -> Result<Tunnel> {
    Tunnel::start(local_port, TunnelProvider::Cloudflare).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require cloudflared to be available
    // Run with --ignored to skip in CI

    #[tokio::test]
    #[ignore = "Requires network and cloudflared"]
    async fn test_cloudflare_tunnel() {
        // Start a dummy server
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Start tunnel
        let tunnel = Tunnel::start(port, TunnelProvider::Cloudflare)
            .await
            .expect("Failed to start tunnel");

        assert!(tunnel.public_url.contains("trycloudflare.com"));

        // Cleanup
        tunnel.stop().await.unwrap();
    }
}
