use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use sha1::{Digest, Sha1};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::{self, pki_types::ServerName};
use tokio_rustls::TlsConnector;

type WsStream = TlsStream<TcpStream>;
const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_HANDSHAKE_HEADER_SIZE: usize = 8192;

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

#[derive(Debug)]
struct WsHandshakeRequest {
    payload: String,
    expected_accept: String,
}

#[derive(Debug, Default)]
struct WsHandshakeResponse {
    status: u16,
    location: Option<String>,
    upgrade: Option<String>,
    connection: Option<String>,
    accept: Option<String>,
    protocol: Option<String>,
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

fn tls_connector() -> TlsConnector {
    let root_store =
        rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

async fn connect_raw(domain: &str, timeout_dur: Duration) -> Result<WsStream, WsError> {
    let connected = timeout(timeout_dur, TcpStream::connect((domain, 443))).await;
    let tcp = match connected {
        Ok(conn) => conn.map_err(WsError::Io)?,
        Err(_) => return Err(WsError::Timeout),
    };

    let server_name = ServerName::try_from(domain.to_string())
        .map_err(|err| WsError::Tls(format!("invalid server name: {err}")))?;
    let connector = tls_connector();
    let tls = timeout(timeout_dur, connector.connect(server_name, tcp)).await;
    match tls {
        Ok(connected) => connected.map_err(|err| WsError::Tls(err.to_string())),
        Err(_) => Err(WsError::Timeout),
    }
}

async fn perform_handshake<S>(mut stream: S, domain: &str) -> Result<(S, WsHandshake), WsError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = build_handshake_request(domain)?;
    stream.write_all(request.payload.as_bytes()).await?;
    stream.flush().await?;

    let header = read_handshake_headers(&mut stream).await?;
    let response = parse_handshake_response(&header)?;
    if response.status == 101 {
        validate_handshake_response(&response, &request.expected_accept)?;
    }

    Ok((
        stream,
        WsHandshake {
            status: response.status,
            redirected: (301..400).contains(&response.status),
            location: response.location,
        },
    ))
}

async fn read_handshake_headers<S>(stream: &mut S) -> Result<Vec<u8>, WsError>
where
    S: AsyncRead + Unpin,
{
    let mut header: Vec<u8> = Vec::new();
    let mut last_two = [0u8; 2];
    while header.len() < MAX_HANDSHAKE_HEADER_SIZE {
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

    if !header.ends_with(b"\r\n\r\n") {
        return Err(WsError::Handshake(
            "oversized_handshake_headers".to_string(),
        ));
    }

    Ok(header)
}

fn parse_handshake_response(header: &[u8]) -> Result<WsHandshakeResponse, WsError> {
    let mut iter = header.split(|b| *b == b'\n');
    let first_line = iter.next().unwrap_or_default();
    let mut response = WsHandshakeResponse {
        status: parse_status(first_line)?,
        ..Default::default()
    };

    for line in iter {
        if line == b"\r" || line.is_empty() {
            break;
        }
        let line = std::str::from_utf8(line)
            .unwrap_or_default()
            .trim_end_matches('\r');
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| WsError::Handshake("malformed_handshake_header".to_string()))?;
        let value = value.trim();

        if name.eq_ignore_ascii_case("location") {
            response.location = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("upgrade") {
            response.upgrade = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("connection") {
            response.connection = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("sec-websocket-accept") {
            response.accept = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("sec-websocket-protocol") {
            response.protocol = Some(value.to_string());
        }
    }

    Ok(response)
}

fn validate_handshake_response(
    response: &WsHandshakeResponse,
    expected_accept: &str,
) -> Result<(), WsError> {
    match response.upgrade.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("websocket") => {}
        _ => return Err(WsError::Handshake("missing_upgrade".to_string())),
    }

    match response.connection.as_deref() {
        Some(value) if has_header_token(value, "upgrade") => {}
        _ => return Err(WsError::Handshake("missing_connection_upgrade".to_string())),
    }

    match response.accept.as_deref() {
        Some(value) if value == expected_accept => {}
        Some(_) => return Err(WsError::Handshake("invalid_ws_accept".to_string())),
        None => return Err(WsError::Handshake("missing_ws_accept".to_string())),
    }

    match response.protocol.as_deref() {
        Some("binary") => Ok(()),
        Some(_) => Err(WsError::Handshake("unexpected_ws_protocol".to_string())),
        None => Err(WsError::Handshake("missing_ws_protocol".to_string())),
    }
}

fn has_header_token(value: &str, token: &str) -> bool {
    value
        .split(',')
        .map(str::trim)
        .any(|part| part.eq_ignore_ascii_case(token))
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

fn build_handshake_request(domain: &str) -> Result<WsHandshakeRequest, WsError> {
    build_handshake_request_with_key(domain, &websocket_key()?)
}

fn build_handshake_request_with_key(
    domain: &str,
    key: &str,
) -> Result<WsHandshakeRequest, WsError> {
    Ok(WsHandshakeRequest {
        payload: format!(
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
        ),
        expected_accept: websocket_accept(key),
    })
}

fn websocket_key() -> Result<String, WsError> {
    let mut key = [0u8; 16];
    getrandom::getrandom(&mut key)
        .map_err(|err| WsError::Handshake(format!("ws key generation failed: {err}")))?;
    Ok(BASE64_STANDARD.encode(key))
}

fn websocket_accept(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WS_GUID);
    BASE64_STANDARD.encode(hasher.finalize())
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
        build_frame, build_handshake_request_with_key, parse_handshake_response, parse_status,
        perform_handshake, read_frame, validate_handshake_response, websocket_accept, WsCloseInfo,
        WsError, WsFrame,
    };
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
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
        let req = build_handshake_request_with_key(domain, "dGhlIHNhbXBsZSBub25jZQ==").unwrap();
        assert!(req.payload.starts_with("GET /apiws HTTP/1.1\r\n"));
        assert!(req.payload.contains(&format!("Host: {domain}\r\n")));
        assert!(req.payload.contains("Upgrade: websocket\r\n"));
        assert!(req.payload.contains("Connection: Upgrade\r\n"));
        assert!(req.payload.contains("Cache-Control: no-cache\r\n"));
        assert!(req.payload.contains("Pragma: no-cache\r\n"));
        assert!(req.payload.contains("Sec-WebSocket-Protocol: binary\r\n"));
        assert!(req
            .payload
            .contains("Accept-Encoding: gzip, deflate, br, zstd\r\n"));
        assert!(req
            .payload
            .contains("Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits\r\n"));
        assert!(req
            .payload
            .contains("User-Agent: Mozilla/5.0 (X11; Linux x86_64)"));
        let key_line = req
            .payload
            .lines()
            .find(|line| line.starts_with("Sec-WebSocket-Key: "))
            .unwrap();
        assert!(key_line.len() > "Sec-WebSocket-Key: ".len());
        assert_eq!(req.expected_accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn websocket_accept_matches_rfc_sample() {
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn strict_handshake_validation_accepts_expected_headers() {
        let req = build_handshake_request_with_key("test.dc.example", "dGhlIHNhbXBsZSBub25jZQ==")
            .unwrap();
        let response = parse_handshake_response(
            b"HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: WebSocket\r\n\
Connection: keep-alive, Upgrade\r\n\
Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
Sec-WebSocket-Protocol: binary\r\n\
\r\n",
        )
        .unwrap();

        validate_handshake_response(&response, &req.expected_accept).unwrap();
    }

    #[test]
    fn strict_handshake_validation_rejects_missing_accept() {
        let req = build_handshake_request_with_key("test.dc.example", "dGhlIHNhbXBsZSBub25jZQ==")
            .unwrap();
        let response = parse_handshake_response(
            b"HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Protocol: binary\r\n\
\r\n",
        )
        .unwrap();

        let err = validate_handshake_response(&response, &req.expected_accept).unwrap_err();
        assert!(matches!(err, WsError::Handshake(reason) if reason == "missing_ws_accept"));
    }

    #[test]
    fn strict_handshake_validation_rejects_connection_without_upgrade_token() {
        let req = build_handshake_request_with_key("test.dc.example", "dGhlIHNhbXBsZSBub25jZQ==")
            .unwrap();
        let response = parse_handshake_response(
            b"HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: keep-alive\r\n\
Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
Sec-WebSocket-Protocol: binary\r\n\
\r\n",
        )
        .unwrap();

        let err = validate_handshake_response(&response, &req.expected_accept).unwrap_err();
        assert!(
            matches!(err, WsError::Handshake(reason) if reason == "missing_connection_upgrade")
        );
    }

    #[test]
    fn strict_handshake_validation_rejects_wrong_protocol() {
        let req = build_handshake_request_with_key("test.dc.example", "dGhlIHNhbXBsZSBub25jZQ==")
            .unwrap();
        let response = parse_handshake_response(
            b"HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
Sec-WebSocket-Protocol: text\r\n\
\r\n",
        )
        .unwrap();

        let err = validate_handshake_response(&response, &req.expected_accept).unwrap_err();
        assert!(matches!(err, WsError::Handshake(reason) if reason == "unexpected_ws_protocol"));
    }

    #[tokio::test]
    async fn perform_handshake_rejects_oversized_headers() {
        let (mut client, mut server) = duplex(32 * 1024);
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            let _ = server.read(&mut buf).await.unwrap();
            let mut response = b"HTTP/1.1 101 Switching Protocols\r\n".to_vec();
            response.extend(std::iter::repeat_n(b'a', 9000));
            server.write_all(&response).await.unwrap();
        });

        let err = perform_handshake(&mut client, "test.dc.example")
            .await
            .unwrap_err();
        assert!(
            matches!(err, WsError::Handshake(reason) if reason == "oversized_handshake_headers")
        );
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
