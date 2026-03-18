use crate::runtime::listen::{bind_host, split_host_port};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

pub async fn serve_health(addr: &str) {
    let Ok((host, port)) = split_host_port(addr) else {
        warn!("failed to parse health endpoint {}", addr);
        return;
    };
    let Ok(listeners) = bind_host(&host, port) else {
        warn!("failed to bind health endpoint {}", addr);
        return;
    };

    let mut tasks = JoinSet::new();
    for (listener, bound_addr) in listeners {
        info!("health endpoint bound to {}", bound_addr);
        tasks.spawn(async move { serve_health_listener(listener).await });
    }

    while let Some(res) = tasks.join_next().await {
        if let Err(err) = res {
            warn!("health listener task failed: {}", err);
        }
    }
}

async fn serve_health_listener(listener: TcpListener) {
    loop {
        match listener.accept().await {
            Ok((mut conn, _)) => {
                let _ = conn
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length:2\r\ncontent-type:text/plain\r\n\r\nOK",
                    )
                    .await;
            }
            Err(err) => {
                warn!("health accept failed: {}", err);
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
}
