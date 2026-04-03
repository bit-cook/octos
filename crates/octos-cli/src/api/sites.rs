use super::AppState;
use crate::site_manager::SiteStatus;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct CreateSiteRequest {
    pub name: String,
    pub subdomain: String,
    /// Profile (user) that owns the site. Defaults to "admin".
    pub profile_id: Option<String>,
    pub title: Option<String>,
}

pub async fn list_sites(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SiteStatus>>, (StatusCode, String)> {
    let mgr = state.site_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "sites not configured".into(),
    ))?;
    mgr.list_sites()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn create_site(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSiteRequest>,
) -> Result<(StatusCode, Json<SiteStatus>), (StatusCode, String)> {
    let mgr = state.site_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "sites not configured".into(),
    ))?;
    let profile_id = req.profile_id.as_deref().unwrap_or("admin");
    let site = mgr
        .create_site(&req.name, &req.subdomain, profile_id, req.title.as_deref())
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let hostname = site.hostname();
    let url = site.url();
    let local_url = format!("http://localhost:{}", site.port);
    let _ = mgr.add_dns_route(&hostname).await;
    let _ = mgr.reload_tunnel().await;
    let status = SiteStatus {
        id: site.id,
        name: site.name,
        subdomain: site.subdomain,
        profile_id: site.profile_id,
        port: site.port,
        url,
        local_url,
        running: true,
        created_at: site.created_at,
    };
    tracing::info!(site = %status.id, url = %status.url, "site created");
    Ok((StatusCode::CREATED, Json(status)))
}

pub async fn start_site(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mgr = state.site_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "sites not configured".into(),
    ))?;
    mgr.start_site(&id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"status": "started"})))
}

pub async fn stop_site(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mgr = state.site_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "sites not configured".into(),
    ))?;
    mgr.stop_site(&id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"status": "stopped"})))
}

pub async fn delete_site(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mgr = state.site_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "sites not configured".into(),
    ))?;
    mgr.delete_site(&id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = mgr.reload_tunnel().await;
    tracing::info!(site = %id, "site deleted");
    Ok(Json(serde_json::json!({"status": "deleted"})))
}

pub async fn reload_tunnel(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mgr = state.site_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "sites not configured".into(),
    ))?;
    let msg = mgr
        .reload_tunnel()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({"status": msg})))
}
