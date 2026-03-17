mod config;
mod modules;
mod runtime;

use crate::config::{telegram_socks_proxy_links, CliConfig};
use crate::runtime::server::run_server;
use crate::runtime::shutdown::wait_for_shutdown_signal;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let cli = CliConfig::parse();
    init_logging(&cli.log_level);

    info!("starting tg-ws-proxy-rs");
    info!("listening on {}:{}", cli.host, cli.port);
    for link in telegram_socks_proxy_links(&cli.host, cli.port) {
        info!("telegram socks proxy link: {}", link);
    }
    if cli.host == "0.0.0.0" {
        info!("for external clients replace 0.0.0.0 with reachable host/IP in the proxy link");
    }

    let server = run_server(cli.clone());

    tokio::select! {
        res = server => {
            match res {
                Ok(()) => info!("server exited"),
                Err(err) => {
                    tracing::error!("server error: {err}");
                }
            }
        }
        _ = wait_for_shutdown_signal() => {
            info!("shutdown signal received");
        }
    }
}

fn init_logging(level: &str) {
    let filter = EnvFilter::new(level);
    tracing_subscriber::fmt::Subscriber::builder()
        .with_env_filter(filter)
        .init();
}
