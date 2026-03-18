//! Profile-local HTTP server for serving files and web content.
//!
//! Each profile can run multiple HTTP servers on dynamically assigned ports,
//! serving content from their `www/` directory. Servers support range requests
//! for large file downloads and can be exposed to the internet via tunnels.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::routing::get;
use chrono::{DateTime, Utc};
use eyre::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::process::Child;
use tokio::sync::RwLock;
use tower_http::services::ServeDir;
use tracing::{debug, error, info, warn};

use super::{Tool, ToolResult};
use crate::tunnel::{Tunnel, TunnelProvider};

/// Information about an active web server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub port: u16,
    pub path: String,
    pub local_url: String,
    pub created_at: DateTime<Utc>,
    pub persistent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<TunnelInfo>,
}

/// Information about an active tunnel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelInfo {
    pub public_url: String,
    pub provider: String,
    pub created_at: DateTime<Utc>,
}

/// Handle to a running server including its tunnel
#[derive(Debug)]
pub struct ServerHandle {
    pub port: u16,
    pub path: String,
    pub shutdown_tx: tokio::sync::oneshot::Sender<()>,
    pub tunnel: Option<TunnelHandle>,
    pub created_at: DateTime<Utc>,
    pub persistent: bool,
}

/// Handle to a running tunnel process
#[derive(Debug)]
pub struct TunnelHandle {
    pub public_url: String,
    pub provider: String,
    pub process: Child,
}

/// Manages HTTP servers for a single profile
#[derive(Debug)]
pub struct ProfileWebServer {
    profile_id: String,
    data_dir: PathBuf,
    servers: Arc<RwLock<HashMap<u16, ServerHandle>>>,
}

impl ProfileWebServer {
    /// Create a new web server manager for a profile
    pub fn new(profile_id: String, data_dir: PathBuf) -> Self {
        Self {
            profile_id,
            data_dir,
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the www directory for this profile
    fn www_dir(&self) -> PathBuf {
        self.data_dir.join("www")
    }

    /// Get the state file path for persistence
    fn state_file(&self) -> PathBuf {
        self.data_dir.join("servers.json")
    }

    /// Find an available port in the range 10000-65535
    async fn find_free_port(&self) -> Result<u16> {
        // Try random ports first to avoid collisions
        for _ in 0..100 {
            let port = rand::random::<u16>() % 55535 + 10000;
            if TcpListener::bind(("127.0.0.1", port)).await.is_ok() {
                return Ok(port);
            }
        }

        // Fallback: sequential scan
        for port in 10000..65535 {
            if TcpListener::bind(("127.0.0.1", port)).await.is_ok() {
                return Ok(port);
            }
        }

        bail!("No free port available in range 10000-65535")
    }

    /// Start a new HTTP server serving files from the given path
    ///
    /// # Arguments
    /// * `path` - Subdirectory under profile's www/ (e.g., "report-2024q3")
    /// * `persistent` - Whether to restore this server after restart
    pub async fn serve(&self, path: &str, persistent: bool) -> Result<ServerInfo> {
        let port = self.find_free_port().await?;
        let www_root = self.www_dir().join(path);

        // Ensure directory exists
        if !www_root.exists() {
            tokio::fs::create_dir_all(&www_root).await?;
        }

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = TcpListener::bind(addr).await?;

        // Build router with ServeDir for static files
        let app = Router::new()
            .nest_service("/", ServeDir::new(&www_root))
            .route("/_health", get(|| async { "OK" }))
            .layer(tower_http::compression::CompressionLayer::new());

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        // Start server in background
        let server = axum::serve(listener, app);
        let server_task = server.with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });

        tokio::spawn(async move {
            if let Err(e) = server_task.await {
                error!("Web server error: {}", e);
            }
        });

        let handle = ServerHandle {
            port,
            path: path.to_string(),
            shutdown_tx,
            tunnel: None,
            created_at: Utc::now(),
            persistent,
        };

        self.servers.write().await.insert(port, handle);

        if persistent {
            self.save_state().await?;
        }

        info!(
            profile = %self.profile_id,
            port = port,
            path = %path,
            "Started web server"
        );

        Ok(ServerInfo {
            port,
            path: path.to_string(),
            local_url: format!("http://127.0.0.1:{}", port),
            created_at: Utc::now(),
            persistent,
            tunnel: None,
        })
    }

    /// Attach a tunnel to an existing server
    pub async fn attach_tunnel(
        &self,
        port: u16,
        public_url: String,
        provider: String,
        mut process: Child,
    ) -> Result<()> {
        let mut servers = self.servers.write().await;

        if let Some(handle) = servers.get_mut(&port) {
            // Kill existing tunnel if present (prevents zombie processes)
            if let Some(mut old_tunnel) = handle.tunnel.take() {
                let _ = old_tunnel.process.kill().await;
                debug!(port = port, "Killed existing tunnel process");
            }

            handle.tunnel = Some(TunnelHandle {
                public_url: public_url.clone(),
                provider: provider.clone(),
                process,
            });

            drop(servers); // Release lock before await
            self.save_state().await?;

            info!(
                profile = %self.profile_id,
                port = port,
                url = %public_url,
                "Attached tunnel to server"
            );

            Ok(())
        } else {
            // Server not found, kill the process to avoid zombie
            let _ = process.kill().await;
            bail!("No server running on port {}", port)
        }
    }

    /// Stop a server and its tunnel
    pub async fn stop(&self, port: u16) -> Result<()> {
        let mut servers = self.servers.write().await;

        if let Some(handle) = servers.remove(&port) {
            // Shutdown HTTP server
            let _ = handle.shutdown_tx.send(());

            // Kill tunnel process if exists
            if let Some(tunnel) = handle.tunnel {
                let mut process = tunnel.process;
                let _ = process.kill().await;
            }

            drop(servers); // Release lock before save
            self.save_state().await?;

            info!(
                profile = %self.profile_id,
                port = port,
                "Stopped web server"
            );

            Ok(())
        } else {
            bail!("No server running on port {}", port)
        }
    }

    /// List all active servers
    pub async fn list(&self) -> Vec<ServerInfo> {
        let servers = self.servers.read().await;

        servers
            .values()
            .map(|h| ServerInfo {
                port: h.port,
                path: h.path.clone(),
                local_url: format!("http://127.0.0.1:{}", h.port),
                created_at: h.created_at,
                persistent: h.persistent,
                tunnel: h.tunnel.as_ref().map(|t| TunnelInfo {
                    public_url: t.public_url.clone(),
                    provider: t.provider.clone(),
                    created_at: h.created_at,
                }),
            })
            .collect()
    }

    /// Get info for a specific server
    pub async fn get(&self, port: u16) -> Option<ServerInfo> {
        let servers = self.servers.read().await;

        servers.get(&port).map(|h| ServerInfo {
            port: h.port,
            path: h.path.clone(),
            local_url: format!("http://127.0.0.1:{}", h.port),
            created_at: h.created_at,
            persistent: h.persistent,
            tunnel: h.tunnel.as_ref().map(|t| TunnelInfo {
                public_url: t.public_url.clone(),
                provider: t.provider.clone(),
                created_at: h.created_at,
            }),
        })
    }

    /// Stop all servers (called on profile shutdown)
    pub async fn stop_all(&self) {
        let mut servers = self.servers.write().await;

        for (port, handle) in servers.drain() {
            let _ = handle.shutdown_tx.send(());
            if let Some(tunnel) = handle.tunnel {
                let mut process = tunnel.process;
                let _ = process.kill().await;
            }
            debug!(port = port, "Stopped server");
        }

        // Don't clear state file - persistent servers should be restored
    }

    /// Save server state to disk
    async fn save_state(&self) -> Result<()> {
        let state: Vec<ServerInfo> = self.list().await;
        let json = serde_json::to_string_pretty(&state)?;

        if let Some(parent) = self.state_file().parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(self.state_file(), json).await?;
        Ok(())
    }

    /// Load and restore persistent servers (called on profile startup)
    pub async fn restore(&self) -> Result<Vec<ServerInfo>> {
        if !self.state_file().exists() {
            return Ok(vec![]);
        }

        let json = tokio::fs::read_to_string(self.state_file()).await?;
        let state: Vec<ServerInfo> = serde_json::from_str(&json)?;

        let mut restored = vec![];

        for info in state {
            if !info.persistent {
                continue;
            }

            // Restart the server
            match self.serve(&info.path, true).await {
                Ok(new_info) => {
                    info!(
                        profile = %self.profile_id,
                        port = new_info.port,
                        path = %info.path,
                        "Restored web server"
                    );
                    restored.push(new_info);
                }
                Err(e) => {
                    warn!(
                        profile = %self.profile_id,
                        path = %info.path,
                        error = %e,
                        "Failed to restore web server"
                    );
                }
            }
        }

        Ok(restored)
    }
}

// ============================================================================
// Agent Tools
// ============================================================================

use serde_json::json;

/// Start a local HTTP server to serve files
pub struct WebServeTool {
    web_server: Arc<ProfileWebServer>,
}

impl WebServeTool {
    pub fn new(web_server: Arc<ProfileWebServer>) -> Self {
        Self { web_server }
    }
}

#[derive(Deserialize)]
struct WebServeInput {
    path: String,
    #[serde(default = "default_true")]
    persistent: bool,
}

fn default_true() -> bool {
    true
}

#[async_trait]
impl Tool for WebServeTool {
    fn name(&self) -> &str {
        "web_serve"
    }

    fn description(&self) -> &str {
        "Start a local HTTP server to serve files from a directory under the profile's www/ folder. \
         Returns a local URL like http://127.0.0.1:xxxxx. \
         Use tunnel_start to expose this to the internet."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Subdirectory under profile's www/ to serve (e.g., 'report-2024q3'). \
                                     Will be created if it doesn't exist."
                },
                "persistent": {
                    "type": "boolean",
                    "description": "Whether to restart this server automatically after profile restart. Default: true.",
                    "default": true
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: WebServeInput = match serde_json::from_value(args.clone()) {
            Ok(i) => i,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("Invalid input: {}", e),
                    success: false,
                    ..Default::default()
                });
            }
        };

        if input.path.contains("..") || input.path.starts_with('/') {
            return Ok(ToolResult {
                output: "Invalid path: cannot contain '..' or start with '/'".to_string(),
                success: false,
                ..Default::default()
            });
        }

        match self.web_server.serve(&input.path, input.persistent).await {
            Ok(info) => {
                let output = format!(
                    "✅ Web server started!\n\n\
                     📍 Local URL: {}\n\
                     📂 Serving: {}\n\
                     🔌 Port: {}\n\n\
                     Next step: Use tunnel_start(port={}) to expose to internet.",
                    info.local_url, info.path, info.port, info.port
                );

                Ok(ToolResult {
                    output,
                    success: true,
                    ..Default::default()
                })
            }
            Err(e) => {
                error!(error = %e, "Failed to start web server");
                Ok(ToolResult {
                    output: format!("Failed to start web server: {}", e),
                    success: false,
                    ..Default::default()
                })
            }
        }
    }
}

/// Start a tunnel to expose a local server to the internet
pub struct TunnelStartTool {
    web_server: Arc<ProfileWebServer>,
}

impl TunnelStartTool {
    pub fn new(web_server: Arc<ProfileWebServer>) -> Self {
        Self { web_server }
    }
}

#[derive(Deserialize)]
struct TunnelStartInput {
    port: u16,
    #[serde(default)]
    provider: String,
    /// Subdomain for custom domain (e.g., "xxx" for xxx.mofa.ai)
    #[serde(default)]
    subdomain: String,
    /// Root domain (default: mofa.ai)
    #[serde(default = "default_domain")]
    domain: String,
    /// Named tunnel name (required when using cloudflare-named provider)
    #[serde(default)]
    tunnel_name: String,
}

fn default_domain() -> String {
    "mofa.ai".to_string()
}

#[async_trait]
impl Tool for TunnelStartTool {
    fn name(&self) -> &str {
        "tunnel_start"
    }

    fn description(&self) -> &str {
        "Expose a local HTTP server to the internet via a secure tunnel. \
         Creates a public HTTPS URL that anyone can access. \
         Requires web_serve to be called first to start the local server. \
         Supports custom domains via cloudflare-named provider (requires pre-created tunnel)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "port": {
                    "type": "integer",
                    "description": "Local port to expose (from web_serve output)"
                },
                "provider": {
                    "type": "string",
                    "enum": ["cloudflare", "cloudflare-named", "ngrok", "auto"],
                    "description": "Tunnel provider. 'cloudflare' (default): free, random domain. 'cloudflare-named': custom domain (xxx.mofa.ai), requires tunnel_name. 'ngrok': requires auth token. 'auto': try cloudflare first.",
                    "default": "cloudflare"
                },
                "subdomain": {
                    "type": "string",
                    "description": "Subdomain for custom domain (e.g., 'dashboard.alice' for dashboard.alice.mofa.ai). Supports multi-level subdomains. Only used with cloudflare-named provider.",
                    "examples": ["dashboard.alice", "api", "report.bob"]
                },
                "domain": {
                    "type": "string",
                    "description": "Root domain for custom domain. Default: mofa.ai",
                    "default": "mofa.ai"
                },
                "tunnel_name": {
                    "type": "string",
                    "description": "Name of pre-created Cloudflare tunnel (required for cloudflare-named provider). Create with: cloudflared tunnel create <name>"
                }
            },
            "required": ["port"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: TunnelStartInput = match serde_json::from_value(args.clone()) {
            Ok(i) => i,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("Invalid input: {}", e),
                    success: false,
                    ..Default::default()
                });
            }
        };

        if input.port == 0 {
            return Ok(ToolResult {
                output: "Port must be specified (use the port from web_serve output)".to_string(),
                success: false,
                ..Default::default()
            });
        }

        if self.web_server.get(input.port).await.is_none() {
            return Ok(ToolResult {
                output: format!(
                    "No server running on port {}. Start a server first with web_serve.",
                    input.port
                ),
                success: false,
                ..Default::default()
            });
        }

        let provider = match input.provider.as_str() {
            "cloudflare" => TunnelProvider::Cloudflare,
            "cloudflare-named" => TunnelProvider::CloudflareNamed,
            "ngrok" => TunnelProvider::Ngrok,
            "auto" => TunnelProvider::Auto,
            "" => TunnelProvider::Cloudflare,
            _ => {
                return Ok(ToolResult {
                    output: format!(
                        "Unknown provider: {}. Use 'cloudflare', 'cloudflare-named', 'ngrok', or 'auto'.",
                        input.provider
                    ),
                    success: false,
                    ..Default::default()
                });
            }
        };

        // Handle named tunnel (custom domain)
        let tunnel_result = if provider == TunnelProvider::CloudflareNamed {
            if input.tunnel_name.is_empty() {
                return Ok(ToolResult {
                    output: "tunnel_name is required for cloudflare-named provider. \
                             Create a tunnel first: cloudflared tunnel create <name>".to_string(),
                    success: false,
                    ..Default::default()
                });
            }
            if input.subdomain.is_empty() {
                return Ok(ToolResult {
                    output: "subdomain is required for cloudflare-named provider (e.g., 'dashboard.alice' for dashboard.alice.mofa.ai)".to_string(),
                    success: false,
                    ..Default::default()
                });
            }

            tokio::time::timeout(
                std::time::Duration::from_secs(60),
                Tunnel::start_named(input.port, &input.tunnel_name, &input.subdomain, &input.domain),
            )
            .await
        } else {
            tokio::time::timeout(
                std::time::Duration::from_secs(45),
                Tunnel::start(input.port, provider),
            )
            .await
        };

        match tunnel_result {
            Ok(Ok(tunnel)) => {
                let public_url = tunnel.url().to_string();

                if let Err(e) = self
                    .web_server
                    .attach_tunnel(
                        input.port,
                        public_url.clone(),
                        provider.as_str().to_string(),
                        tunnel.process,
                    )
                    .await
                {
                    return Ok(ToolResult {
                        output: format!("Tunnel started but failed to attach: {}", e),
                        success: false,
                        ..Default::default()
                    });
                }

                let output = format!(
                    "🌐 Tunnel established!\n\n\
                     🔗 Public URL: {}\n\
                     📍 Local: http://127.0.0.1:{}\n\n\
                     Share this link with anyone - it's accessible from anywhere. \
                     Note: This link expires when the server is stopped.",
                    public_url, input.port
                );

                Ok(ToolResult {
                    output,
                    success: true,
                    ..Default::default()
                })
            }
            Ok(Err(e)) => {
                error!(error = %e, "Failed to start tunnel");
                let troubleshooting = if provider == TunnelProvider::CloudflareNamed {
                    format!(
                        "Failed to start named tunnel: {}\n\n\
                         Troubleshooting for named tunnels:\
                         - Ensure tunnel '{}' exists: cloudflared tunnel list\n\
                         - Check credentials file exists at ~/.cloudflared/{}.json\n\
                         - Verify DNS record exists: cloudflared tunnel route dns {} {}.{}\n\
                         - Make sure you're logged in: cloudflared tunnel login",
                        e, input.tunnel_name, input.tunnel_name, input.tunnel_name, input.subdomain, input.domain
                    )
                } else {
                    format!(
                        "Failed to start tunnel: {}\n\n\
                         Troubleshooting:\
                         - Make sure cloudflared is installed or can be downloaded\n\
                         - Check your internet connection\n\
                         - Try a different port (some may be blocked)",
                        e
                    )
                };
                Ok(ToolResult {
                    output: troubleshooting,
                    success: false,
                    ..Default::default()
                })
            }
            Err(_) => {
                error!("Tunnel startup timeout");
                Ok(ToolResult {
                    output: "Tunnel startup timed out after 45 seconds.".to_string(),
                    success: false,
                    ..Default::default()
                })
            }
        }
    }
}

/// Stop a tunnel or web server
pub struct TunnelStopTool {
    web_server: Arc<ProfileWebServer>,
}

impl TunnelStopTool {
    pub fn new(web_server: Arc<ProfileWebServer>) -> Self {
        Self { web_server }
    }
}

#[derive(Deserialize)]
struct TunnelStopInput {
    port: u16,
}

#[async_trait]
impl Tool for TunnelStopTool {
    fn name(&self) -> &str {
        "tunnel_stop"
    }

    fn description(&self) -> &str {
        "Stop a web server and its tunnel. Frees up the port and invalidates the public URL."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "port": {
                    "type": "integer",
                    "description": "Port of the server to stop (from web_serve output)"
                }
            },
            "required": ["port"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: TunnelStopInput = match serde_json::from_value(args.clone()) {
            Ok(i) => i,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("Invalid input: {}", e),
                    success: false,
                    ..Default::default()
                });
            }
        };

        match self.web_server.stop(input.port).await {
            Ok(()) => Ok(ToolResult {
                output: format!(
                    "✅ Server on port {} stopped.\n\
                     The public URL is no longer accessible.",
                    input.port
                ),
                success: true,
                ..Default::default()
            }),
            Err(e) => Ok(ToolResult {
                output: format!("Failed to stop server: {}", e),
                success: false,
                ..Default::default()
            }),
        }
    }
}

/// List active web servers and tunnels
pub struct WebStatusTool {
    web_server: Arc<ProfileWebServer>,
}

impl WebStatusTool {
    pub fn new(web_server: Arc<ProfileWebServer>) -> Self {
        Self { web_server }
    }
}

#[async_trait]
impl Tool for WebStatusTool {
    fn name(&self) -> &str {
        "web_status"
    }

    fn description(&self) -> &str {
        "List all active web servers and their tunnels for this profile. \
         Shows local URLs, public URLs (if tunneled), and port numbers."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: &serde_json::Value) -> Result<ToolResult> {
        let servers = self.web_server.list().await;

        if servers.is_empty() {
            return Ok(ToolResult {
                output: "No active web servers.\n\nUse web_serve to start one.".to_string(),
                success: true,
                ..Default::default()
            });
        }

        let mut lines = vec!["📡 Active web servers:".to_string()];

        for (i, server) in servers.iter().enumerate() {
            let tunnel_info = if let Some(ref tunnel) = server.tunnel {
                format!("\n   🌐 {}", tunnel.public_url)
            } else {
                "\n   (no tunnel - use tunnel_start to expose)".to_string()
            };

            lines.push(format!(
                "\n{}. Port {} → {}{}",
                i + 1,
                server.port,
                server.path,
                tunnel_info
            ));
        }

        lines.push(format!("\n\nTotal: {} server(s)", servers.len()));

        Ok(ToolResult {
            output: lines.join(""),
            success: true,
            ..Default::default()
        })
    }
}

/// Publish a file to web server - copy to www/ and serve it
pub struct WebPublishTool {
    web_server: Arc<ProfileWebServer>,
    cwd: PathBuf,
}

impl WebPublishTool {
    pub fn new(web_server: Arc<ProfileWebServer>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            web_server,
            cwd: cwd.into(),
        }
    }
}

#[derive(Deserialize)]
struct WebPublishInput {
    /// Source file path (relative to working directory)
    source_file: String,
    /// Target subdirectory under www/ (e.g., "share" or "report-2024q3")
    /// If not specified, uses the source filename
    #[serde(default)]
    target_dir: String,
    /// Whether to restart the server if already running
    #[serde(default = "default_true")]
    persistent: bool,
}

#[async_trait]
impl Tool for WebPublishTool {
    fn name(&self) -> &str {
        "web_publish"
    }

    fn description(&self) -> &str {
        "Publish a file to a web server for easy viewing/downloading. \
         Copies the file to the profile's www/ folder and starts a local HTTP server. \
         Returns a local URL that can be shared or used with tunnel_start for public access."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "source_file": {
                    "type": "string",
                    "description": "Path to the file to publish (relative to working directory)"
                },
                "target_dir": {
                    "type": "string",
                    "description": "Subdirectory under www/ to publish to. Defaults to the source filename (without extension)",
                    "default": ""
                },
                "persistent": {
                    "type": "boolean",
                    "description": "Whether to keep the server running after restart. Default: true",
                    "default": true
                }
            },
            "required": ["source_file"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: WebPublishInput = match serde_json::from_value(args.clone()) {
            Ok(i) => i,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("Invalid input: {}", e),
                    success: false,
                    ..Default::default()
                });
            }
        };

        // Validate source file path
        let source_path = self.cwd.join(&input.source_file);
        if !source_path.exists() {
            return Ok(ToolResult {
                output: format!("Source file not found: {}", input.source_file),
                success: false,
                ..Default::default()
            });
        }
        if !source_path.is_file() {
            return Ok(ToolResult {
                output: format!("Source path is not a file: {}", input.source_file),
                success: false,
                ..Default::default()
            });
        }

        // Determine target directory
        let target_dir = if input.target_dir.is_empty() {
            // Use filename without extension as default directory
            source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("share")
                .to_string()
        } else {
            input.target_dir
        };

        // Validate target_dir (no .. or /)
        if target_dir.contains("..") || target_dir.starts_with('/') {
            return Ok(ToolResult {
                output: "Invalid target_dir: cannot contain '..' or start with '/'".to_string(),
                success: false,
                ..Default::default()
            });
        }

        // Get www directory and target path
        let www_dir = self.web_server.www_dir();
        let target_path = www_dir.join(&target_dir);
        let target_file = target_path.join(source_path.file_name().unwrap_or("file".as_ref()));

        // Copy file to www directory
        if let Err(e) = tokio::fs::create_dir_all(&target_path).await {
            return Ok(ToolResult {
                output: format!("Failed to create directory: {}", e),
                success: false,
                ..Default::default()
            });
        }

        match tokio::fs::copy(&source_path, &target_file).await {
            Ok(bytes) => {
                debug!(bytes = bytes, "Copied file to www directory");
            }
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("Failed to copy file: {}", e),
                    success: false,
                    ..Default::default()
                });
            }
        }

        // Start or get existing web server for this directory
        let info = match self.web_server.serve(&target_dir, input.persistent).await {
            Ok(info) => info,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("Failed to start web server: {}", e),
                    success: false,
                    ..Default::default()
                });
            }
        };

        let filename = source_path.file_name().unwrap_or("file".as_ref()).to_string_lossy();
        let file_url = format!("{}/{}", info.local_url, filename);

        let output = format!(
            "✅ File published successfully!\n\n\
             📄 File: {}\n\
             📂 Published to: {}/{}\n\n\
             🔗 Access URLs:\n\
             • Direct: {}\n\
             • Browse: {}\n\n\
             To expose publicly: tunnel_start(port={})",
            input.source_file,
            target_dir,
            filename,
            file_url,
            info.local_url,
            info.port
        );

        Ok(ToolResult {
            output,
            success: true,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_find_free_port() {
        let tmp = TempDir::new().unwrap();
        let mgr = ProfileWebServer::new("test".to_string(), tmp.path().to_path_buf());

        let port = mgr.find_free_port().await.unwrap();
        assert!(port >= 10000);
        // u16 max is 65535, so no need to check upper bound
    }

    #[tokio::test]
    async fn test_serve_and_stop() {
        let tmp = TempDir::new().unwrap();
        let mgr = ProfileWebServer::new("test".to_string(), tmp.path().to_path_buf());

        // Create test file
        let www_dir = tmp.path().join("www/test");
        tokio::fs::create_dir_all(&www_dir).await.unwrap();
        tokio::fs::write(www_dir.join("hello.txt"), "Hello World")
            .await
            .unwrap();

        // Start server
        let info = mgr.serve("test", false).await.unwrap();
        assert!(info.port >= 10000);

        // Verify it's running
        let list = mgr.list().await;
        assert_eq!(list.len(), 1);

        // Stop server
        mgr.stop(info.port).await.unwrap();

        let list = mgr.list().await;
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_http_request() {
        let tmp = TempDir::new().unwrap();
        let mgr = ProfileWebServer::new("test".to_string(), tmp.path().to_path_buf());

        // Create test file
        let www_dir = tmp.path().join("www/test_http");
        tokio::fs::create_dir_all(&www_dir).await.unwrap();
        tokio::fs::write(www_dir.join("index.html"), "<h1>Hello Crew</h1>")
            .await
            .unwrap();

        // Start server
        let info = mgr.serve("test_http", false).await.unwrap();

        // Make HTTP request
        let client = reqwest::Client::new();
        let resp = client
            .get(&format!("http://127.0.0.1:{}/index.html", info.port))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("Hello Crew"));

        // Cleanup
        mgr.stop(info.port).await.unwrap();
    }

    #[tokio::test]
    async fn test_tool_execution() {
        let tmp = TempDir::new().unwrap();
        let mgr = Arc::new(ProfileWebServer::new(
            "test".to_string(),
            tmp.path().to_path_buf(),
        ));

        // Create test file
        let www_dir = tmp.path().join("www/tool_test");
        tokio::fs::create_dir_all(&www_dir).await.unwrap();
        tokio::fs::write(www_dir.join("data.json"), r#"{"msg": "hi"}"#)
            .await
            .unwrap();

        // Test web_serve tool
        let tool = WebServeTool::new(mgr.clone());
        let result = tool
            .execute(&serde_json::json!({"path": "tool_test"}))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("http://127.0.0.1:"));

        // Test web_status tool
        let status_tool = WebStatusTool::new(mgr.clone());
        let result = status_tool.execute(&serde_json::json!({})).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("tool_test"));

        // Cleanup
        mgr.list().await.iter().for_each(|s| {
            let _ = mgr.stop(s.port);
        });
    }
}
