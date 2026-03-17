use clap::Parser;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Parser, Serialize, Deserialize)]
#[command(name = "tg-ws-proxy-rs")]
#[command(version = "0.1.0")]
#[command(about = "SOCKS5 proxy for Telegram over Telegram WebSocket endpoints", long_about = None)]
pub struct CliConfig {
    #[arg(long, default_value = "0.0.0.0")]
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

    #[arg(long = "health-addr", default_value = "0.0.0.0:8080")]
    pub health_addr: String,
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
    let server = if host.contains(':') && !host.starts_with('[') && !host.ends_with(']') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("tg://socks?server={server}&port={port}")
}

pub fn telegram_socks_proxy_links(host: &str, port: u16) -> Vec<String> {
    vec![telegram_socks_proxy_link(host, port)]
}

#[derive(Debug, Clone)]
pub struct RoutedConfig {
    pub read_timeout_secs: u64,
    pub connect_timeout_secs: u64,
}

impl From<&CliConfig> for RoutedConfig {
    fn from(cfg: &CliConfig) -> Self {
        Self {
            read_timeout_secs: cfg.read_timeout_secs,
            connect_timeout_secs: cfg.connect_timeout_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::telegram_socks_proxy_link;
    use super::CliConfig;
    use clap::Parser;

    #[test]
    fn cli_defaults_to_listen_on_all_interfaces() {
        let cfg = CliConfig::parse_from(["tg-ws-proxy-rs"]);
        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.health_addr, "0.0.0.0:8080");
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
}
