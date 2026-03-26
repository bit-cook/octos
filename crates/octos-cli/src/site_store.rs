use chrono::{DateTime, Utc};
use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub subdomain: String,
    /// Profile (user) that owns this site.
    pub profile_id: String,
    pub path: PathBuf,
    pub port: u16,
    pub tunnel_domain: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Site {
    /// Full hostname: `{subdomain}.{profile_id}.{tunnel_domain}`
    pub fn hostname(&self) -> String {
        format!("{}.{}.{}", self.subdomain, self.profile_id, self.tunnel_domain)
    }

    /// Public URL.
    pub fn url(&self) -> String {
        format!("https://{}", self.hostname())
    }
}

pub struct SiteStore {
    sites_dir: PathBuf,
    www_dir: PathBuf,
}

impl SiteStore {
    pub fn open(data_dir: &Path) -> Result<Self> {
        let sites_dir = data_dir.join("sites");
        let www_dir = data_dir.join("www");
        fs::create_dir_all(&sites_dir).wrap_err("creating sites dir")?;
        fs::create_dir_all(&www_dir).wrap_err("creating www dir")?;
        Ok(Self { sites_dir, www_dir })
    }

    pub fn www_dir(&self) -> &Path {
        &self.www_dir
    }

    pub fn list(&self) -> Result<Vec<Site>> {
        let mut sites = Vec::new();
        let entries = match fs::read_dir(&self.sites_dir) {
            Ok(e) => e,
            Err(_) => return Ok(sites),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match self.read_site_file(&path) {
                Ok(site) => sites.push(site),
                Err(e) => tracing::warn!("skipping invalid site file {:?}: {}", path, e),
            }
        }
        sites.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(sites)
    }

    pub fn get(&self, id: &str) -> Result<Option<Site>> {
        let path = self.site_path(id);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(self.read_site_file(&path)?))
    }

    pub fn save(&self, site: &Site) -> Result<()> {
        validate_site_id(&site.id)?;
        let path = self.site_path(&site.id);
        let json = serde_json::to_string_pretty(site)?;
        fs::write(&path, json).wrap_err("writing site file")?;
        let site_www = self.www_dir.join(&site.id);
        fs::create_dir_all(&site_www).wrap_err("creating site www dir")?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let path = self.site_path(id);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path)?;
        let site_www = self.www_dir.join(id);
        if site_www.exists() {
            fs::remove_dir_all(&site_www)?;
        }
        Ok(true)
    }

    pub fn next_available_port(&self, base: u16) -> Result<u16> {
        let sites = self.list()?;
        let used: std::collections::HashSet<u16> = sites.iter().map(|s| s.port).collect();
        let mut port = base;
        while used.contains(&port) {
            port += 1;
        }
        Ok(port)
    }

    fn site_path(&self, id: &str) -> PathBuf {
        self.sites_dir.join(format!("{}.json", id))
    }

    fn read_site_file(&self, path: &Path) -> Result<Site> {
        let data = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&data)?)
    }
}

fn validate_site_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 {
        eyre::bail!("site id must be 1-64 characters");
    }
    if !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        eyre::bail!("site id may only contain lowercase letters, digits, and hyphens");
    }
    if id.starts_with('-') || id.ends_with('-') {
        eyre::bail!("site id must not start or end with a hyphen");
    }
    Ok(())
}
