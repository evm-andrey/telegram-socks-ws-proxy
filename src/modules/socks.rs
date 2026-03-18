use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocksUser {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocksAuthConfig {
    pub users: Vec<SocksUser>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SocksCommand {
    Connect {
        target_host: String,
        target_port: u16,
    },
}

#[derive(Debug)]
pub enum SocksError {
    UnsupportedVersion,
    NoAcceptableMethod,
    AuthenticationFailed,
    UnsupportedCommand,
    UnsupportedAddressType,
    Io(io::Error),
    ParseError(String),
}

impl From<io::Error> for SocksError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

pub fn parse_socks_auth(input: &str) -> Result<Option<SocksAuthConfig>, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let mut users = Vec::new();
    for entry in raw
        .split(|c| c == ',' || c == '\n' || c == ';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (username, password) = entry
            .split_once(':')
            .ok_or_else(|| format!("invalid SOCKS auth entry `{entry}`: expected user:pass"))?;
        if username.is_empty() {
            return Err("SOCKS auth username cannot be empty".to_string());
        }
        if password.is_empty() {
            return Err(format!(
                "SOCKS auth password cannot be empty for user `{username}`"
            ));
        }
        users.push(SocksUser {
            username: username.to_string(),
            password: password.to_string(),
        });
    }

    if users.is_empty() {
        return Ok(None);
    }

    Ok(Some(SocksAuthConfig { users }))
}

pub async fn handle_socks5_handshake<S>(
    stream: &mut S,
    auth: Option<&SocksAuthConfig>,
) -> Result<SocksCommand, SocksError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut hdr = [0u8; 2];
    stream.read_exact(&mut hdr).await?;
    if hdr[0] != 0x05 {
        return Err(SocksError::UnsupportedVersion);
    }
    let nmethods = hdr[1] as usize;
    let mut methods = vec![0u8; nmethods];
    stream.read_exact(&mut methods).await?;
    let selected_method = if auth.is_some() { 0x02 } else { 0x00 };
    if !methods.contains(&selected_method) {
        stream.write_all(&[0x05, 0xFF]).await?;
        return Err(SocksError::NoAcceptableMethod);
    }
    stream.write_all(&[0x05, selected_method]).await?;

    if let Some(auth) = auth {
        authenticate_user(stream, auth).await?;
    }

    let mut req = [0u8; 4];
    stream.read_exact(&mut req).await?;
    if req[0] != 0x05 || req[1] != 0x01 || req[2] != 0x00 {
        let _ = stream
            .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        return Err(SocksError::UnsupportedCommand);
    }

    let target = read_target_address(stream, req[3]).await?;
    let mut port = [0u8; 2];
    stream.read_exact(&mut port).await?;
    let target_port = u16::from_be_bytes(port);

    let reply = [0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
    stream.write_all(&reply).await?;

    Ok(SocksCommand::Connect {
        target_host: target,
        target_port,
    })
}

async fn authenticate_user<S>(stream: &mut S, auth: &SocksAuthConfig) -> Result<(), SocksError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut version = [0u8; 1];
    stream.read_exact(&mut version).await?;
    if version[0] != 0x01 {
        let _ = stream.write_all(&[0x01, 0x01]).await;
        return Err(SocksError::AuthenticationFailed);
    }

    let username = read_auth_field(stream).await?;
    let password = read_auth_field(stream).await?;
    let accepted = auth
        .users
        .iter()
        .any(|user| user.username == username && user.password == password);

    let status = if accepted { 0x00 } else { 0x01 };
    stream.write_all(&[0x01, status]).await?;
    if accepted {
        Ok(())
    } else {
        Err(SocksError::AuthenticationFailed)
    }
}

async fn read_auth_field<S>(stream: &mut S) -> Result<String, SocksError>
where
    S: AsyncRead + Unpin,
{
    let mut len = [0u8; 1];
    stream.read_exact(&mut len).await?;
    let mut value = vec![0u8; len[0] as usize];
    stream.read_exact(&mut value).await?;
    String::from_utf8(value).map_err(|err| SocksError::ParseError(err.to_string()))
}

async fn read_target_address<S>(stream: &mut S, atyp: u8) -> Result<String, SocksError>
where
    S: AsyncRead + Unpin,
{
    match atyp {
        0x01 => {
            let mut addr = [0u8; 4];
            stream.read_exact(&mut addr).await?;
            let ip = Ipv4Addr::from(addr);
            Ok(ip.to_string())
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut domain = vec![0u8; len[0] as usize];
            stream.read_exact(&mut domain).await?;
            String::from_utf8(domain).map_err(|e| SocksError::ParseError(e.to_string()))
        }
        0x04 => {
            let mut addr = [0u8; 16];
            stream.read_exact(&mut addr).await?;
            let ip = Ipv6Addr::from(addr);
            Ok(ip.to_string())
        }
        _ => Err(SocksError::UnsupportedAddressType),
    }
}

pub fn format_socks5_fail_reply(reason: u8) -> Vec<u8> {
    vec![0x05, reason, 0x00, 0x01, 0, 0, 0, 0, 0, 0]
}

pub fn is_ipv6(dst: &str) -> bool {
    dst.contains(':')
}

#[cfg(test)]
mod tests {
    use super::{handle_socks5_handshake, is_ipv6, parse_socks_auth, SocksError};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn parse_socks5_connect_ipv4() {
        let (a, b) = UnixStream::pair().unwrap();
        let client = a;
        let mut server = b;

        let writer = tokio::spawn(async move {
            let mut w = client;
            w.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap();
            let mut select = [0u8; 2];
            w.read_exact(&mut select).await.unwrap();
            assert_eq!(select, [0x05, 0x00]);

            let req = vec![0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1, 0x1F, 0x90];
            w.write_all(&req).await.unwrap();

            let mut ack = [0u8; 10];
            w.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack[0], 0x05);
            assert_eq!(ack[1], 0x00);
        });

        let parsed = handle_socks5_handshake(&mut server, None).await.unwrap();
        assert_eq!(
            parsed,
            super::SocksCommand::Connect {
                target_host: "127.0.0.1".to_string(),
                target_port: 8080
            }
        );
        writer.await.unwrap();
    }

    #[test]
    fn ipv6_check() {
        assert!(is_ipv6("2001:db8::1"));
        assert!(!is_ipv6("8.8.8.8"));
    }

    #[tokio::test]
    async fn parse_socks5_connect_with_auth() {
        let (a, b) = UnixStream::pair().unwrap();
        let client = a;
        let mut server = b;
        let auth = parse_socks_auth("alice:secret").unwrap().unwrap();

        let writer = tokio::spawn(async move {
            let mut w = client;
            w.write_all(&[0x05, 0x01, 0x02]).await.unwrap();
            let mut select = [0u8; 2];
            w.read_exact(&mut select).await.unwrap();
            assert_eq!(select, [0x05, 0x02]);

            w.write_all(&[0x01, 0x05]).await.unwrap();
            w.write_all(b"alice").await.unwrap();
            w.write_all(&[0x06]).await.unwrap();
            w.write_all(b"secret").await.unwrap();

            let mut auth_reply = [0u8; 2];
            w.read_exact(&mut auth_reply).await.unwrap();
            assert_eq!(auth_reply, [0x01, 0x00]);

            let req = vec![0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1, 0x1F, 0x90];
            w.write_all(&req).await.unwrap();

            let mut ack = [0u8; 10];
            w.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack[0], 0x05);
            assert_eq!(ack[1], 0x00);
        });

        let parsed = handle_socks5_handshake(&mut server, Some(&auth))
            .await
            .unwrap();
        assert_eq!(
            parsed,
            super::SocksCommand::Connect {
                target_host: "127.0.0.1".to_string(),
                target_port: 8080
            }
        );
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_auth_credentials() {
        let (a, b) = UnixStream::pair().unwrap();
        let client = a;
        let mut server = b;
        let auth = parse_socks_auth("alice:secret").unwrap().unwrap();

        let writer = tokio::spawn(async move {
            let mut w = client;
            w.write_all(&[0x05, 0x01, 0x02]).await.unwrap();
            let mut select = [0u8; 2];
            w.read_exact(&mut select).await.unwrap();
            assert_eq!(select, [0x05, 0x02]);

            w.write_all(&[0x01, 0x05]).await.unwrap();
            w.write_all(b"alice").await.unwrap();
            w.write_all(&[0x05]).await.unwrap();
            w.write_all(b"wrong").await.unwrap();

            let mut auth_reply = [0u8; 2];
            w.read_exact(&mut auth_reply).await.unwrap();
            assert_eq!(auth_reply, [0x01, 0x01]);
        });

        let err = handle_socks5_handshake(&mut server, Some(&auth))
            .await
            .unwrap_err();
        assert!(matches!(err, SocksError::AuthenticationFailed));
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_when_client_does_not_offer_auth_method() {
        let (a, b) = UnixStream::pair().unwrap();
        let client = a;
        let mut server = b;
        let auth = parse_socks_auth("alice:secret").unwrap().unwrap();

        let writer = tokio::spawn(async move {
            let mut w = client;
            w.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
            let mut select = [0u8; 2];
            w.read_exact(&mut select).await.unwrap();
            assert_eq!(select, [0x05, 0xFF]);
        });

        let err = handle_socks5_handshake(&mut server, Some(&auth))
            .await
            .unwrap_err();
        assert!(matches!(err, SocksError::NoAcceptableMethod));
        writer.await.unwrap();
    }

    #[test]
    fn parse_multiple_socks_users_from_env() {
        let auth = parse_socks_auth("alice:one,bob:two\ncarol:three")
            .unwrap()
            .unwrap();
        assert_eq!(auth.users.len(), 3);
        assert_eq!(auth.users[1].username, "bob");
        assert_eq!(auth.users[1].password, "two");
    }
}
