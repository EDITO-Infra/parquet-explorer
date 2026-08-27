use std::{net::{IpAddr, Ipv4Addr, Ipv6Addr}, path::Path};

use anyhow::{Context, Result, bail};
use tokio::net::lookup_host;
use url::Url;

use crate::config::Settings;

pub async fn validate_source_uri(input: &str, settings: &Settings) -> Result<String> {
    let mut url = Url::parse(input).context("source must be an absolute URL")?;
    let scheme = url.scheme().to_ascii_lowercase();
    if !settings.allowed_source_schemes.contains(&scheme) {
        bail!("source scheme '{scheme}' is not allowed");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("credentials embedded in source URLs are not allowed");
    }

    match scheme.as_str() {
        "http" | "https" => validate_remote(&url, settings).await?,
        "file" => {
            if !settings.allow_local_files { bail!("local file sources are disabled"); }
            let path = url.to_file_path().map_err(|_| anyhow::anyhow!("invalid file URL"))?;
            let canonical = tokio::fs::canonicalize(&path).await
                .with_context(|| format!("could not resolve local path {}", path.display()))?;
            let root = tokio::fs::canonicalize(&settings.local_data_root).await
                .with_context(|| format!("could not resolve PV_LOCAL_DATA_ROOT {}", settings.local_data_root.display()))?;
            if !is_within(&canonical, &root) { bail!("local file is outside PV_LOCAL_DATA_ROOT"); }
            url = Url::from_file_path(canonical).map_err(|_| anyhow::anyhow!("could not normalize file URL"))?;
        }
        _ => {}
    }
    Ok(url.to_string())
}

async fn validate_remote(url: &Url, settings: &Settings) -> Result<()> {
    let host = url.host_str().context("remote URL has no host")?;
    if !settings.allowed_remote_hosts.is_empty() && !host_allowed(host, &settings.allowed_remote_hosts) {
        bail!("remote host '{host}' is not allowlisted");
    }
    if settings.allow_private_networks { return Ok(()); }

    let port = url.port_or_known_default().context("URL scheme has no known port")?;
    let addresses = lookup_host((host, port)).await.with_context(|| format!("could not resolve '{host}'"))?;
    let mut found = false;
    for address in addresses {
        found = true;
        if is_special_ip(address.ip()) { bail!("remote host resolves to a private or special-use address"); }
    }
    if !found { bail!("remote host did not resolve to any address"); }
    Ok(())
}

fn host_allowed(host: &str, patterns: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    patterns.iter().any(|pattern| {
        let pattern = pattern.to_ascii_lowercase();
        if let Some(suffix) = pattern.strip_prefix("*.") {
            host.ends_with(&format!(".{suffix}")) && host != suffix
        } else { host == pattern }
    })
}

fn is_within(path: &Path, root: &Path) -> bool { path == root || path.starts_with(root) }

fn is_special_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => special_v4(ip),
        IpAddr::V6(ip) => special_v6(ip),
    }
}

fn special_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_broadcast() || ip.is_documentation()
        || ip.is_multicast() || ip.is_unspecified()
        || ip.octets()[0] == 0
        || ip.octets()[0] >= 224
        || matches!(ip.octets(), [100, 64..=127, _, _])
        || matches!(ip.octets(), [198, 18..=19, _, _])
}

fn special_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || ip.is_multicast()
        || (ip.segments()[0] & 0xfe00) == 0xfc00  // unique local fc00::/7
        || (ip.segments()[0] & 0xffc0) == 0xfe80  // link local fe80::/10
        || (ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8) // documentation
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_allowlist() {
        assert!(host_allowed("tiles.example.org", &["*.example.org".into()]));
        assert!(!host_allowed("example.org", &["*.example.org".into()]));
    }
    #[test]
    fn rejects_private_ips() {
        assert!(is_special_ip("127.0.0.1".parse().unwrap()));
        assert!(is_special_ip("10.2.3.4".parse().unwrap()));
        assert!(!is_special_ip("8.8.8.8".parse().unwrap()));
    }
}
