use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use std::io;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_native_tls::{native_tls, TlsConnector, TlsStream};

type WsStream = TlsStream<TcpStream>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsCloseInfo {
    pub code: Option<u16>,
    pub reason: Option<String>,
}

#[derive(Debug, Error)]
pub enum WsError {
    #[error("ws handshake failed: {0}")]
    Handshake(String),
    #[error("ws handshake failed: not upgraded: {status}")]
    HttpStatus {
        status: u16,
        location: Option<String>,
    },
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("tls: {0}")]
    Tls(String),
    #[error("timeout")]
    Timeout,
}

#[derive(Debug)]
pub struct WsHandshake {
    pub status: u16,
    pub redirected: bool,
    pub location: Option<String>,
}

#[derive(Debug, PartialEq)]
enum WsFrame {
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(WsCloseInfo),
    Other(Vec<u8>),
}

pub struct RawWsClient {
    stream: WsStream,
}

pub struct RawWsReader {
    stream: tokio::io::ReadHalf<WsStream>,
}

pub struct RawWsWriter {
    stream: tokio::io::WriteHalf<WsStream>,
}

pub enum WsIncoming {
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Close(WsCloseInfo),
}

impl RawWsClient {
    pub async fn connect(domain: &str, timeout_dur: Duration) -> Result<Self, WsError> {
        let stream = connect_raw(domain, timeout_dur).await?;
        let (stream, hs) = perform_handshake(stream, domain).await?;
        if hs.status != 101 {
            return Err(WsError::HttpStatus {
                status: hs.status,
                location: hs.location,
            });
        }
        Ok(Self { stream })
    }

    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>, WsError> {
        loop {
            match read_frame(&mut self.stream).await? {
                Some(WsFrame::Binary(payload)) => return Ok(Some(payload)),
                Some(WsFrame::Ping(payload)) => {
                    let frame = build_frame_with_opcode(0xA, &payload);
                    self.stream.write_all(&frame).await?;
                    self.stream.flush().await?;
                }
                Some(WsFrame::Pong(_)) => {}
                Some(WsFrame::Close(_)) => return Ok(None),
                Some(WsFrame::Other(_)) => return Ok(None),
                None => return Ok(None),
            }
        }
    }

    pub async fn send(&mut self, payload: &[u8]) -> Result<(), WsError> {
        let frame = build_frame(payload);
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), WsError> {
        self.stream.shutdown().await?;
        Ok(())
    }

    pub fn split(self) -> (RawWsReader, RawWsWriter) {
        let (reader, writer) = tokio::io::split(self.stream);
        (
            RawWsReader { stream: reader },
            RawWsWriter { stream: writer },
        )
    }

    pub fn into_inner(self) -> WsStream {
        self.stream
    }
}

impl RawWsReader {
    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>, WsError> {
        loop {
            match self.recv_frame().await? {
                Some(WsIncoming::Binary(payload)) => return Ok(Some(payload)),
                Some(WsIncoming::Ping(_)) => {}
                Some(WsIncoming::Close(_)) => return Ok(None),
                None => return Ok(None),
            }
        }
    }

    pub async fn recv_frame(&mut self) -> Result<Option<WsIncoming>, WsError> {
        loop {
            match read_frame(&mut self.stream).await? {
                Some(WsFrame::Binary(payload)) => return Ok(Some(WsIncoming::Binary(payload))),
                Some(WsFrame::Ping(payload)) => return Ok(Some(WsIncoming::Ping(payload))),
                Some(WsFrame::Pong(_)) => {}
                Some(WsFrame::Close(info)) => return Ok(Some(WsIncoming::Close(info))),
                Some(WsFrame::Other(_)) => {
                    return Ok(Some(WsIncoming::Close(WsCloseInfo {
                        code: None,
                        reason: Some("unexpected_ws_frame".to_string()),
                    })))
                }
                None => return Ok(None),
            }
        }
    }
}

impl RawWsWriter {
    pub async fn send(&mut self, payload: &[u8]) -> Result<(), WsError> {
        let frame = build_frame(payload);
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), WsError> {
        self.stream.shutdown().await?;
        Ok(())
    }

    pub async fn send_control_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), WsError> {
        let frame = build_frame_with_opcode(opcode, payload);
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;
        Ok(())
    }
}

impl WsError {
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::HttpStatus { status, .. } => Some(*status),
            _ => None,
        }
    }
}

pub async fn handshake(_ip: &str, domain: &str) -> Result<WsHandshake, WsError> {
    let stream = connect_raw(domain, Duration::from_secs(5)).await?;
    let (_, hs) = perform_handshake(stream, domain).await?;
    Ok(hs)
}

fn tls_connector() -> Result<native_tls::TlsConnector, WsError> {
    native_tls::TlsConnector::builder()
        .build()
        .map_err(|err| WsError::Tls(err.to_string()))
}

async fn connect_raw(domain: &str, timeout_dur: Duration) -> Result<WsStream, WsError> {
    let connected = timeout(timeout_dur, TcpStream::connect((domain, 443))).await;
    let tcp = match connected {
        Ok(conn) => conn.map_err(WsError::Io)?,
        Err(_) => return Err(WsError::Timeout),
    };

    let connector = TlsConnector::from(tls_connector()?);
    let tls = timeout(timeout_dur, connector.connect(domain, tcp)).await;
    match tls {
        Ok(connected) => connected.map_err(|err| WsError::Tls(err.to_string())),
        Err(_) => Err(WsError::Timeout),
    }
}

async fn perform_handshake<S>(mut stream: S, domain: &str) -> Result<(S, WsHandshake), WsError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream
        .write_all(build_handshake_request(domain)?.as_bytes())
        .await?;
    stream.flush().await?;

    let mut header: Vec<u8> = Vec::new();
    let mut last_two = [0u8; 2];
    while header.len() < 8192 {
        let mut b = [0u8; 1];
        let read = stream.read(&mut b).await?;
        if read == 0 {
            return Err(WsError::Handshake("connection closed".to_string()));
        }
        header.push(b[0]);
        let len = header.len();
        if len >= 2 {
            last_two.copy_from_slice(&header[(len - 2)..len]);
            if last_two == *b"\r\n" && len >= 4 && header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
    }

    let mut iter = header.split(|b| *b == b'\n');
    let first_line = iter.next().unwrap_or_default();
    let status = parse_status(first_line)?;
    let mut location = None;
    for line in iter {
        if line == b"\r" || line.is_empty() {
            break;
        }
        let line = std::str::from_utf8(line).unwrap_or_default();
        if let Some(value) = line.to_lowercase().strip_prefix("location:") {
            location = Some(value.trim().to_string());
        }
    }

    Ok((
        stream,
        WsHandshake {
            status,
            redirected: (301..400).contains(&status),
            location,
        },
    ))
}

fn parse_status(line: &[u8]) -> Result<u16, WsError> {
    let line = String::from_utf8_lossy(line);
    let mut parts = line.split_whitespace();
    let _version = parts.next();
    let status = parts
        .next()
        .ok_or_else(|| WsError::Handshake("bad response".to_string()))?;
    status
        .parse::<u16>()
        .map_err(|_| WsError::Handshake("bad status".to_string()))
}

fn build_handshake_request(domain: &str) -> Result<String, WsError> {
    let key = websocket_key()?;
    Ok(format!(
        "GET /apiws HTTP/1.1\r\n\
Host: {domain}\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Cache-Control: no-cache\r\n\
Pragma: no-cache\r\n\
Sec-WebSocket-Key: {key}\r\n\
Sec-WebSocket-Version: 13\r\n\
Sec-WebSocket-Protocol: binary\r\n\
Origin: https://web.telegram.org\r\n\
Accept-Encoding: gzip, deflate, br, zstd\r\n\
User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36\r\n\
Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits\r\n\
\r\n"
    ))
}

fn websocket_key() -> Result<String, WsError> {
    let mut key = [0u8; 16];
    getrandom::getrandom(&mut key)
        .map_err(|err| WsError::Handshake(format!("ws key generation failed: {err}")))?;
    Ok(BASE64_STANDARD.encode(key))
}

fn build_frame(payload: &[u8]) -> Vec<u8> {
    build_frame_with_opcode(0x2, payload)
}

fn build_frame_with_opcode(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 16);
    out.push(0x80 | (opcode & 0x0F));
    if payload.len() < 126 {
        out.push(0x80 | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        out.push(0x80 | 126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }

    let mut key = [0u8; 4];
    if getrandom::getrandom(&mut key).is_err() {
        key = [0x11, 0x22, 0x33, 0x44];
    }
    out.extend_from_slice(&key);
    for (idx, b) in payload.iter().enumerate() {
        out.push(*b ^ key[idx % 4]);
    }
    out
}

async fn read_frame<S>(stream: &mut S) -> Result<Option<WsFrame>, WsError>
where
    S: AsyncRead + Unpin,
{
    let mut hdr = [0u8; 2];
    if let Err(err) = stream.read_exact(&mut hdr).await {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(WsError::Io(err));
    }

    let opcode = hdr[0] & 0x0f;
    let is_binary_opcode = matches!(opcode, 0x0 | 0x1 | 0x2);
    let is_ping = opcode == 0x9;
    let is_pong = opcode == 0xA;

    let mut len = (hdr[1] & 0x7f) as u64;
    if len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext).await?;
        len = u16::from_be_bytes(ext) as u64;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext).await?;
        len = u64::from_be_bytes(ext);
    }

    let payload_len =
        usize::try_from(len).map_err(|_| WsError::Handshake("frame too large".to_string()))?;
    let masked = (hdr[1] & 0x80) != 0;
    let mut mask = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask).await?;
    }

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload).await?;

    if masked {
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
    }

    if opcode == 0x8 {
        let close = parse_close_payload(&payload);
        return Ok(Some(WsFrame::Close(close)));
    }
    if is_ping {
        return Ok(Some(WsFrame::Ping(payload)));
    }
    if is_pong {
        return Ok(Some(WsFrame::Pong(payload)));
    }
    if is_binary_opcode {
        return Ok(Some(WsFrame::Binary(payload)));
    }
    Ok(Some(WsFrame::Other(payload)))
}

fn parse_close_payload(payload: &[u8]) -> WsCloseInfo {
    if payload.len() < 2 {
        return WsCloseInfo {
            code: None,
            reason: None,
        };
    }

    let code = Some(u16::from_be_bytes([payload[0], payload[1]]));
    let reason = if payload.len() > 2 {
        Some(String::from_utf8_lossy(&payload[2..]).to_string())
    } else {
        None
    };

    WsCloseInfo { code, reason }
}

#[cfg(test)]
mod tests {
    use super::{
        build_frame, build_handshake_request, parse_status, read_frame, WsCloseInfo, WsFrame,
    };
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    #[test]
    fn parse_status_code() {
        let code = parse_status(b"HTTP/1.1 101 Switching Protocols").unwrap();
        assert_eq!(code, 101);
    }

    #[test]
    fn frame_format_has_mask_and_opcode() {
        let frame = build_frame(b"abc");
        assert_eq!(frame[0], 0x82);
        assert!(frame[1] & 0x80 != 0);
    }

    #[test]
    fn handshake_request_contains_domain_header() {
        let domain = "test.dc.example";
        let req = build_handshake_request(domain).unwrap();
        assert!(req.starts_with("GET /apiws HTTP/1.1\r\n"));
        assert!(req.contains(&format!("Host: {domain}\r\n")));
        assert!(req.contains("Upgrade: websocket\r\n"));
        assert!(req.contains("Connection: Upgrade\r\n"));
        assert!(req.contains("Cache-Control: no-cache\r\n"));
        assert!(req.contains("Pragma: no-cache\r\n"));
        assert!(req.contains("Sec-WebSocket-Protocol: binary\r\n"));
        assert!(req.contains("Accept-Encoding: gzip, deflate, br, zstd\r\n"));
        assert!(req
            .contains("Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits\r\n"));
        assert!(req.contains("User-Agent: Mozilla/5.0 (X11; Linux x86_64)"));
        let key_line = req
            .lines()
            .find(|line| line.starts_with("Sec-WebSocket-Key: "))
            .unwrap();
        assert!(key_line.len() > "Sec-WebSocket-Key: ".len());
    }

    #[tokio::test]
    async fn read_masked_frame() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut c, _) = listener.accept().await.unwrap();
            let payload = b"pong";
            let mask = [0x11, 0x22, 0x33, 0x44];
            let mut frame = Vec::with_capacity(payload.len() + 6);
            frame.push(0x82);
            frame.push(0x80 | payload.len() as u8);
            frame.extend_from_slice(&mask);
            for (i, b) in payload.iter().enumerate() {
                frame.push(*b ^ mask[i % 4]);
            }
            c.write_all(&frame).await.unwrap();
            c.shutdown().await.unwrap();
        });

        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let payload = read_frame(&mut stream).await.unwrap();
        assert_eq!(payload, Some(WsFrame::Binary(b"pong".to_vec())));
    }

    #[tokio::test]
    async fn read_ping_then_binary() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut c, _) = listener.accept().await.unwrap();
            let ping = vec![0x89, 0x03, b'a', b'b', b'c'];
            c.write_all(&ping).await.unwrap();
            let bin = build_frame(b"ok");
            c.write_all(&bin).await.unwrap();
            c.shutdown().await.unwrap();
        });

        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let ping = read_frame(&mut stream).await.unwrap();
        let binary = read_frame(&mut stream).await.unwrap();
        assert!(matches!(ping, Some(WsFrame::Ping(_))));
        assert_eq!(binary, Some(WsFrame::Binary(b"ok".to_vec())));
    }

    #[tokio::test]
    async fn read_close_frame_with_code() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut c, _) = listener.accept().await.unwrap();
            let frame = vec![0x88, 0x02, 0x03, 0xE8];
            c.write_all(&frame).await.unwrap();
            c.shutdown().await.unwrap();
        });

        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let frame = read_frame(&mut stream).await.unwrap();
        assert_eq!(
            frame,
            Some(WsFrame::Close(WsCloseInfo {
                code: Some(1000),
                reason: None,
            }))
        );
    }
}
