use crate::config::CliConfig;
use crate::config::RoutedConfig;
use crate::modules::health;
use crate::modules::routing::route_client;
use crate::runtime::listen::bind_host;
use tokio::net::TcpListener;
use tokio::task::{JoinHandle, JoinSet};
use tracing::{debug, info};

pub async fn run_server(config: CliConfig) -> std::io::Result<()> {
    let routing_config = std::sync::Arc::new(
        RoutedConfig::try_from(&config)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?,
    );
    if let Some(auth) = routing_config.socks_auth.as_ref() {
        info!(
            "socks authentication enabled for {} account(s)",
            auth.users.len()
        );
    }
    let listeners = bind_host(&config.host, config.port)?;
    for (_, addr) in &listeners {
        info!("proxy bound to {}", addr);
    }

    let _health: JoinHandle<()> = tokio::spawn(async move {
        health::serve_health(&config.health_addr).await;
    });

    let mut tasks = JoinSet::new();
    for (listener, addr) in listeners {
        let routing_config = routing_config.clone();
        tasks.spawn(async move { accept_loop(listener, addr, routing_config).await });
    }

    match tasks.join_next().await {
        Some(Ok(res)) => res,
        Some(Err(err)) => Err(std::io::Error::other(err.to_string())),
        None => Ok(()),
    }
}

async fn accept_loop(
    listener: TcpListener,
    bound_addr: std::net::SocketAddr,
    routing_config: std::sync::Arc<RoutedConfig>,
) -> std::io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        debug!("accept {} on {}", peer, bound_addr);
        let routing_config = routing_config.clone();
        tokio::spawn(async move {
            route_client(stream, peer, routing_config).await;
        });
    }
}
