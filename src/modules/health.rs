use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use tracing::warn;

pub async fn serve_health(addr: &str) {
    let Ok(listener) = TcpListener::bind(addr).await else {
        warn!("failed to bind health endpoint {}", addr);
        return;
    };
    loop {
        if let Ok((mut conn, _)) = listener.accept().await {
            let _ = conn
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length:2\r\ncontent-type:text/plain\r\n\r\nOK",
                )
                .await;
        }
        sleep(Duration::from_millis(1)).await;
    }
}
