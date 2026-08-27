use std::{collections::HashSet, env, net::{IpAddr, SocketAddr}, path::PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Settings {
    pub bind: SocketAddr,
    pub cors_origins: Vec<String>,
    pub allowed_source_schemes: HashSet<String>,
    pub allowed_remote_hosts: Vec<String>,
    pub allow_private_networks: bool,
    pub allow_local_files: bool,
    pub local_data_root: PathBuf,
    pub max_page_size: usize,
    pub max_spatial_features: usize,
    pub batch_size: usize,
    pub max_open_datasets: usize,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let host = env::var("PV_HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port = env::var("PV_PORT").unwrap_or_else(|_| "8080".into()).parse::<u16>()
            .context("PV_PORT must be a valid TCP port")?;
        let ip = host.parse::<IpAddr>().context("PV_HOST must be an IP address")?;

        Ok(Self {
            bind: SocketAddr::new(ip, port),
            cors_origins: csv("PV_CORS_ORIGINS", "http://localhost:5173,http://localhost:3000"),
            allowed_source_schemes: csv("PV_ALLOWED_SOURCE_SCHEMES", "https,http")
                .into_iter().map(|s| s.to_ascii_lowercase()).collect(),
            allowed_remote_hosts: csv("PV_ALLOWED_REMOTE_HOSTS", ""),
            allow_private_networks: boolean("PV_ALLOW_PRIVATE_NETWORKS", false),
            allow_local_files: boolean("PV_ALLOW_LOCAL_FILES", false),
            local_data_root: PathBuf::from(env::var("PV_LOCAL_DATA_ROOT").unwrap_or_else(|_| "/data".into())),
            max_page_size: number("PV_MAX_PAGE_SIZE", 25_000)?,
            max_spatial_features: number("PV_MAX_SPATIAL_FEATURES", 100_000)?,
            batch_size: number("PV_BATCH_SIZE", 8_192)?,
            max_open_datasets: number("PV_MAX_OPEN_DATASETS", 512)?,
        })
    }
}

fn csv(name: &str, default: &str) -> Vec<String> {
    env::var(name).unwrap_or_else(|_| default.into())
        .split(',').map(str::trim).filter(|v| !v.is_empty()).map(ToOwned::to_owned).collect()
}

fn boolean(name: &str, default: bool) -> bool {
    env::var(name).ok().and_then(|v| match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }).unwrap_or(default)
}

fn number<T>(name: &str, default: T) -> Result<T>
where T: std::str::FromStr, T::Err: std::error::Error + Send + Sync + 'static {
    match env::var(name) {
        Ok(v) => v.parse::<T>().with_context(|| format!("{name} has an invalid value")),
        Err(_) => Ok(default),
    }
}
