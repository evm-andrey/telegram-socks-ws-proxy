use crate::config::CliConfig;
use crate::config::RoutedConfig;
use crate::modules::health;
use crate::modules::routing::route_client;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{debug, info};

pub async fn run_server(config: CliConfig) -> std::io::Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("proxy bound to {}", addr);
    let routing_config = std::sync::Arc::new(RoutedConfig::from(&config));

    let _health: JoinHandle<()> = tokio::spawn(async move {
        health::serve_health(&config.health_addr).await;
    });

    loop {
        let (stream, peer) = listener.accept().await?;
        debug!("accept {}", peer);
        let routing_config = routing_config.clone();
        tokio::spawn(async move {
            route_client(stream, peer, routing_config).await;
        });
    }
}
