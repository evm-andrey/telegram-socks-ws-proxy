use crate::modules::telegram::MtProtoMessageSplitter;
use crate::modules::ws::{RawWsClient, WsIncoming};
use std::io;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::info;

struct BridgeStats {
    bytes: u64,
    packets: u64,
    end_reason: String,
}

pub async fn bridge_ws_tcp(
    client: TcpStream,
    ws: RawWsClient,
    init: Vec<u8>,
    splitter: Option<MtProtoMessageSplitter>,
    session: &str,
) -> io::Result<()> {
    let started = Instant::now();
    let (mut ws_reader, ws_writer) = ws.split();
    let ws_writer = Arc::new(Mutex::new(ws_writer));
    {
        let mut ws = ws_writer.lock().await;
        ws.send(&init).await.map_err(io::Error::other)?;
    }

    let (mut client_reader, mut client_writer) = client.into_split();
    let (c2ws_res, w2c_res) = tokio::join!(
        async {
            let mut buf = vec![0u8; 32 * 1024];
            let mut splitter = splitter;
            let mut up_bytes = 0u64;
            let mut up_packets = 0u64;
            let end_reason = "client_eof".to_string();
            loop {
                let n = client_reader.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                up_bytes += n as u64;
                up_packets += 1;
                let mut ws = ws_writer.lock().await;
                if let Some(splitter) = splitter.as_mut() {
                    let parts = splitter.split(&buf[..n]);
                    for part in parts {
                        ws.send(&part).await.map_err(io::Error::other)?;
                    }
                } else {
                    ws.send(&buf[..n]).await.map_err(io::Error::other)?;
                }
            }
            {
                let mut ws = ws_writer.lock().await;
                ws.close().await.map_err(io::Error::other)?;
            }
            Ok::<BridgeStats, io::Error>(BridgeStats {
                bytes: up_bytes,
                packets: up_packets,
                end_reason,
            })
        },
        async {
            let mut down_bytes = 0u64;
            let mut down_packets = 0u64;
            let mut end_reason = "ws_eof".to_string();
            loop {
                match ws_reader
                    .recv_frame()
                    .await
                    .map_err(|err| io::Error::other(err.to_string()))?
                {
                    Some(WsIncoming::Binary(data)) => {
                        down_bytes += data.len() as u64;
                        down_packets += 1;
                        client_writer.write_all(&data).await?;
                    }
                    Some(WsIncoming::Ping(payload)) => {
                        let mut ws = ws_writer.lock().await;
                        ws.send_control_frame(0xA, &payload)
                            .await
                            .map_err(io::Error::other)?;
                    }
                    Some(WsIncoming::Close(info)) => {
                        end_reason = format!(
                            "ws_close(code={},reason={})",
                            info.code
                                .map(|code| code.to_string())
                                .unwrap_or_else(|| "none".to_string()),
                            info.reason.unwrap_or_else(|| "none".to_string())
                        );
                        break;
                    }
                    None => break,
                }
            }
            client_writer.shutdown().await?;
            Ok::<BridgeStats, io::Error>(BridgeStats {
                bytes: down_bytes,
                packets: down_packets,
                end_reason,
            })
        },
    );

    let up = c2ws_res?;
    let down = w2c_res?;
    info!(
        "ws bridge closed {} duration_ms={} up_bytes={} up_packets={} up_end={} down_bytes={} down_packets={} down_end={}",
        session,
        started.elapsed().as_millis(),
        up.bytes,
        up.packets,
        up.end_reason,
        down.bytes,
        down.packets,
        down.end_reason
    );
    Ok(())
}

pub async fn bridge_tcp_tcp(
    client: TcpStream,
    remote_addr: &str,
    remote_port: u16,
) -> io::Result<()> {
    bridge_tcp_tcp_with_prelude(client, remote_addr, remote_port, &[]).await
}

pub async fn bridge_tcp_tcp_with_prelude(
    mut client: TcpStream,
    remote_addr: &str,
    remote_port: u16,
    prelude: &[u8],
) -> io::Result<()> {
    let mut remote = TcpStream::connect((remote_addr, remote_port)).await?;
    if !prelude.is_empty() {
        remote.write_all(prelude).await?;
    }

    let _ = tokio::io::copy_bidirectional(&mut client, &mut remote).await?;
    client.shutdown().await?;
    remote.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{bridge_tcp_tcp, bridge_tcp_tcp_with_prelude};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn tcp_to_tcp_bridge() {
        let echo_listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind echo");
        let echo_addr = echo_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut remote, _) = echo_listener.accept().await.unwrap();
            let mut data = [0u8; 12];
            remote.read_exact(&mut data).await.unwrap();
            remote.write_all(&data).await.unwrap();
        });

        let local_listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind local");
        let local_addr = local_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (client, _) = local_listener.accept().await.unwrap();
            bridge_tcp_tcp(client, &echo_addr.ip().to_string(), echo_addr.port())
                .await
                .unwrap();
        });

        let mut client = tokio::net::TcpStream::connect(local_addr)
            .await
            .expect("connect local");
        client.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[tokio::test]
    async fn tcp_to_tcp_bridge_preserves_prelude() {
        let echo_listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind echo");
        let echo_addr = echo_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut remote, _) = echo_listener.accept().await.unwrap();
            let mut data = [0u8; 12];
            remote.read_exact(&mut data).await.unwrap();
            remote.write_all(&data).await.unwrap();
        });

        let local_listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind local");
        let local_addr = local_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (client, _) = local_listener.accept().await.unwrap();
            bridge_tcp_tcp_with_prelude(
                client,
                &echo_addr.ip().to_string(),
                echo_addr.port(),
                b"init",
            )
            .await
            .unwrap();
        });

        let mut client = tokio::net::TcpStream::connect(local_addr)
            .await
            .expect("connect local");
        client.write_all(b"-payload").await.unwrap();
        let mut buf = [0u8; 12];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"init-payload");
    }
}
