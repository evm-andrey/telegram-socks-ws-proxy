use clap::Parser;
use if_addrs::get_if_addrs;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv6Addr};

use crate::modules::socks::{parse_socks_auth, SocksAuthConfig};

#[derive(Clone, Debug, Parser, Serialize, Deserialize)]
#[command(name = "tg-ws-proxy-rs")]
#[command(version)]
#[command(about = "SOCKS5 proxy for Telegram over Telegram WebSocket endpoints", long_about = None)]
pub struct CliConfig {
    #[arg(long, default_value = "::")]
    pub host: String,

    #[arg(short = 'p', long, default_value_t = 1080)]
    pub port: u16,

    #[arg(short = 'l', long, default_value = "info")]
    pub log_level: String,

    #[arg(long = "read-timeout", default_value_t = 15)]
    pub read_timeout_secs: u64,

    #[arg(long = "connect-timeout", default_value_t = 10)]
    pub connect_timeout_secs: u64,

    #[arg(long, default_value = "/config/config.toml")]
    #[serde(default)]
    pub config_file: String,

    #[arg(long = "health-addr", default_value = "[::]:8080")]
    pub health_addr: String,

    #[arg(
        long = "socks-users",
        env = "SOCKS_USERS",
        default_value = "",
        hide_env_values = true
    )]
    #[serde(default)]
    pub socks_users: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub host: String,
    pub port: u16,
    pub log_level: String,
    pub ws_pool_size: usize,
    pub ws_pool_ttl_secs: u64,
    pub read_timeout_secs: u64,
    pub connect_timeout_secs: u64,
    pub health_addr: String,
}

pub fn telegram_socks_proxy_link(host: &str, port: u16) -> String {
    let host = normalize_host(host);
    let server = if host.contains(':') && !host.starts_with('[') && !host.ends_with(']') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("tg://socks?server={server}&port={port}")
}

pub fn telegram_socks_proxy_links(host: &str, port: u16) -> Vec<String> {
    link_addrs(host)
        .into_iter()
        .map(|addr| telegram_socks_proxy_link(&addr, port))
        .collect()
}

#[derive(Debug, Clone)]
pub struct RoutedConfig {
    pub read_timeout_secs: u64,
    pub connect_timeout_secs: u64,
    pub socks_auth: Option<SocksAuthConfig>,
}

impl TryFrom<&CliConfig> for RoutedConfig {
    type Error = String;

    fn try_from(cfg: &CliConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            read_timeout_secs: cfg.read_timeout_secs,
            connect_timeout_secs: cfg.connect_timeout_secs,
            socks_auth: parse_socks_auth(&cfg.socks_users)?,
        })
    }
}

fn link_addrs(host: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut addrs = Vec::new();

    let normalized = normalize_host(host).to_string();
    if is_unspecified_host(&normalized) {
        for discovered in discover_link_addrs() {
            if seen.insert(discovered.clone()) {
                addrs.push(discovered);
            }
        }
        if addrs.is_empty() {
            addrs.push(normalized);
        }
        return addrs;
    }

    addrs.push(normalized);
    addrs
}

fn discover_link_addrs() -> Vec<String> {
    let Ok(ifaces) = get_if_addrs() else {
        return Vec::new();
    };

    let mut ranked = Vec::new();
    let mut seen = HashSet::new();
    for iface in ifaces {
        let ip = iface.ip();
        let host = ip.to_string();
        if !seen.insert(host.clone()) {
            continue;
        }
        let score = announce_priority(ip);
        if score < 255 {
            ranked.push((score, host));
        }
    }

    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let mut addrs: Vec<String> = ranked.into_iter().map(|(_, host)| host).collect();
    if addrs.is_empty() {
        addrs.push("127.0.0.1".to_string());
        addrs.push("::1".to_string());
    }
    addrs
}

fn announce_priority(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V6(v6) if is_public_ipv6(&v6) => 0,
        IpAddr::V4(v4) if !v4.is_loopback() => 1,
        IpAddr::V6(v6)
            if !v6.is_loopback() && !v6.is_unspecified() && !v6.is_unicast_link_local() =>
        {
            2
        }
        IpAddr::V4(v4) if v4.is_loopback() => 3,
        IpAddr::V6(v6) if v6.is_loopback() => 4,
        _ => 255,
    }
}

fn is_public_ipv6(ip: &Ipv6Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_unique_local()
        && !ip.is_unicast_link_local()
}

fn is_unspecified_host(host: &str) -> bool {
    normalize_host(host)
        .parse::<IpAddr>()
        .map(|ip| ip.is_unspecified())
        .unwrap_or(false)
}

fn normalize_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::CliConfig;
    use super::{telegram_socks_proxy_link, RoutedConfig};
    use clap::Parser;

    #[test]
    fn cli_defaults_to_listen_on_all_interfaces() {
        let cfg = CliConfig::parse_from(["tg-ws-proxy-rs"]);
        assert_eq!(cfg.host, "::");
        assert_eq!(cfg.health_addr, "[::]:8080");
    }

    #[test]
    fn telegram_link_formats_ipv4_host() {
        assert_eq!(
            telegram_socks_proxy_link("127.0.0.1", 1080),
            "tg://socks?server=127.0.0.1&port=1080"
        );
    }

    #[test]
    fn telegram_link_formats_ipv6_host() {
        assert_eq!(
            telegram_socks_proxy_link("2001:db8::1", 1080),
            "tg://socks?server=[2001:db8::1]&port=1080"
        );
    }

    #[test]
    fn routed_config_parses_socks_users() {
        let cfg = CliConfig::parse_from(["tg-ws-proxy-rs", "--socks-users", "alice:one,bob:two"]);
        let routed = RoutedConfig::try_from(&cfg).unwrap();
        let auth = routed.socks_auth.expect("expected auth config");
        assert_eq!(auth.users.len(), 2);
        assert_eq!(auth.users[0].username, "alice");
        assert_eq!(auth.users[1].password, "two");
    }
}
