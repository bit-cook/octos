use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use eyre::{Result, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::RwLock;

use crate::site_store::{Site, SiteStore};

const DEFAULT_DOMAIN: &str = "octos-cloud.org";
const TUNNEL_NAME: &str = "octos";
const PORT_BASE: u16 = 10000;

struct SiteProcess {
    #[allow(dead_code)]
    port: u16,
    #[allow(dead_code)]
    started_at: DateTime<Utc>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

struct TunnelProcess {
    #[allow(dead_code)]
    pid: u32,
    #[allow(dead_code)]
    started_at: DateTime<Utc>,
    stop_tx: tokio::sync::watch::Sender<bool>,
}

pub struct SiteManager {
    running: Arc<RwLock<HashMap<String, SiteProcess>>>,
    store: Arc<SiteStore>,
    tunnel: Arc<RwLock<Option<TunnelProcess>>>,
    domain: String,
    data_dir: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SiteStatus {
    pub id: String,
    pub name: String,
    pub subdomain: String,
    pub profile_id: String,
    pub port: u16,
    pub url: String,
    pub local_url: String,
    pub running: bool,
    pub created_at: DateTime<Utc>,
}

impl SiteManager {
    pub fn new(store: Arc<SiteStore>, data_dir: &Path) -> Self {
        Self {
            running: Arc::new(RwLock::new(HashMap::new())),
            store,
            tunnel: Arc::new(RwLock::new(None)),
            domain: DEFAULT_DOMAIN.to_string(),
            data_dir: data_dir.to_path_buf(),
        }
    }

    pub async fn list_sites(&self) -> Result<Vec<SiteStatus>> {
        let sites = self.store.list()?;
        let running = self.running.read().await;
        Ok(sites
            .into_iter()
            .map(|s| {
                let url = s.url();
                SiteStatus {
                    running: running.contains_key(&s.id),
                    url,
                    local_url: format!("http://localhost:{}", s.port),
                    profile_id: s.profile_id.clone(),
                    id: s.id,
                    name: s.name,
                    subdomain: s.subdomain,
                    port: s.port,
                    created_at: s.created_at,
                }
            })
            .collect())
    }

    pub async fn create_site(
        &self,
        name: &str,
        subdomain: &str,
        profile_id: &str,
        title: Option<&str>,
    ) -> Result<Site> {
        let id = name.to_lowercase().replace(' ', "-");
        if self.store.get(&id)?.is_some() {
            bail!("site '{}' already exists", id);
        }
        let port = self.store.next_available_port(PORT_BASE)?;
        let www_path = self.store.www_dir().join(&id);
        std::fs::create_dir_all(&www_path)?;

        let display_title = title.unwrap_or(name);
        let html = generate_default_html(display_title, subdomain, &self.domain, port);
        std::fs::write(www_path.join("index.html"), html)?;

        let site = Site {
            id: id.clone(),
            name: name.to_string(),
            subdomain: subdomain.to_string(),
            profile_id: profile_id.to_string(),
            path: www_path,
            port,
            tunnel_domain: self.domain.clone(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.store.save(&site)?;
        self.start_site(&id).await?;
        Ok(site)
    }

    pub async fn delete_site(&self, id: &str) -> Result<()> {
        let _ = self.stop_site(id).await;
        self.store.delete(id)?;
        Ok(())
    }

    pub async fn start_site(&self, id: &str) -> Result<()> {
        {
            let running = self.running.read().await;
            if running.contains_key(id) {
                return Ok(());
            }
        }
        let site = self
            .store
            .get(id)?
            .ok_or_else(|| eyre::eyre!("site '{}' not found", id))?;
        let serve_dir = site.path.clone();
        let port = site.port;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let site_id = id.to_string();

        tokio::spawn(async move {
            use axum::Router;
            use tower_http::services::ServeDir;
            let app = Router::new().fallback_service(ServeDir::new(&serve_dir));
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!(site = %site_id, port, "failed to bind: {}", e);
                    return;
                }
            };
            tracing::info!(site = %site_id, port, "site server started");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
            tracing::info!(site = %site_id, "site server stopped");
        });

        let mut running = self.running.write().await;
        running.insert(
            id.to_string(),
            SiteProcess {
                port,
                started_at: Utc::now(),
                shutdown_tx,
            },
        );
        Ok(())
    }

    pub async fn stop_site(&self, id: &str) -> Result<()> {
        let mut running = self.running.write().await;
        if let Some(proc) = running.remove(id) {
            let _ = proc.shutdown_tx.send(());
        }
        Ok(())
    }

    pub fn generate_tunnel_config(&self, tunnel_id: &str, creds_file: &str) -> Result<String> {
        let sites = self.store.list()?;
        let mut ingress = String::new();
        for site in &sites {
            ingress.push_str(&format!(
                "  - hostname: {}\n    service: http://localhost:{}\n",
                site.hostname(),
                site.port
            ));
        }
        ingress.push_str("  - service: http_status:404\n");
        Ok(format!(
            "tunnel: {tunnel_id}\ncredentials-file: {creds_file}\n\ningress:\n{ingress}"
        ))
    }

    pub async fn reload_tunnel(&self) -> Result<String> {
        let config_dir = self.data_dir.join("cloudflared");
        std::fs::create_dir_all(&config_dir)?;
        let config_path = config_dir.join("config.yml");
        if !config_path.exists() {
            return Ok("no tunnel configured yet".into());
        }
        let (tunnel_id, creds_file) = parse_tunnel_config(&std::fs::read_to_string(&config_path)?);
        if tunnel_id.is_empty() {
            return Ok("no tunnel ID in config".into());
        }
        let config = self.generate_tunnel_config(&tunnel_id, &creds_file)?;
        std::fs::write(&config_path, &config)?;
        self.stop_tunnel().await?;
        self.start_tunnel(&config_path).await?;
        Ok("tunnel reloaded".into())
    }

    pub async fn add_dns_route(&self, hostname: &str) -> Result<String> {
        let fqdn = hostname.to_string();
        let output = Command::new("cloudflared")
            .args(["tunnel", "route", "dns", TUNNEL_NAME, &fqdn])
            .output()
            .await?;
        let out = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() {
            Ok(out)
        } else {
            bail!("cloudflared route dns failed: {}", out)
        }
    }

    async fn start_tunnel(&self, config_path: &Path) -> Result<()> {
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let mut child = Command::new("cloudflared")
            .args([
                "tunnel",
                "--no-autoupdate",
                "--config",
                &config_path.to_string_lossy(),
                "run",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let pid = child.id().unwrap_or(0);
        tracing::info!(pid, "cloudflared tunnel started");
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "cloudflared", "{}", line);
                }
            });
        }
        tokio::spawn(async move {
            tokio::select! {
                _ = child.wait() => tracing::warn!("cloudflared exited"),
                _ = stop_rx.changed() => { let _ = child.kill().await; tracing::info!("cloudflared killed"); }
            }
        });
        *self.tunnel.write().await = Some(TunnelProcess {
            pid,
            started_at: Utc::now(),
            stop_tx,
        });
        Ok(())
    }

    pub async fn stop_tunnel(&self) -> Result<()> {
        if let Some(proc) = self.tunnel.write().await.take() {
            let _ = proc.stop_tx.send(true);
        }
        Ok(())
    }

    pub async fn start_all(&self) -> Result<()> {
        for site in self.store.list()? {
            if let Err(e) = self.start_site(&site.id).await {
                tracing::warn!(site = %site.id, "failed to start site: {}", e);
            }
        }
        Ok(())
    }
}

fn parse_tunnel_config(content: &str) -> (String, String) {
    let mut tid = String::new();
    let mut creds = String::new();
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("tunnel:") {
            tid = v.trim().to_string();
        }
        if let Some(v) = line.strip_prefix("credentials-file:") {
            creds = v.trim().to_string();
        }
    }
    (tid, creds)
}

fn generate_default_html(title: &str, subdomain: &str, domain: &str, port: u16) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0"><title>{title} - {domain}</title>
<style>*{{margin:0;padding:0;box-sizing:border-box}}body{{font-family:system-ui,sans-serif;background:linear-gradient(135deg,#6366f1,#1e293b);min-height:100vh;display:flex;align-items:center;justify-content:center;color:#fff}}.card{{background:rgba(255,255,255,.12);backdrop-filter:blur(10px);border-radius:20px;padding:60px;text-align:center;max-width:500px}}h1{{font-size:2.5rem;margin-bottom:12px}}p{{font-size:1.1rem;opacity:.85}}.badge{{display:inline-block;margin-top:16px;background:rgba(255,255,255,.18);padding:6px 18px;border-radius:16px;font-size:.85rem}}</style></head>
<body><div class="card"><h1>{title}</h1><p>{subdomain}.{domain}</p><div class="badge">port {port}</div></div></body></html>"#
    )
}
