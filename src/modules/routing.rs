mod backoff;
mod ipv6_cache;

use crate::config::RoutedConfig;
use crate::modules::relay::{bridge_tcp_tcp, bridge_tcp_tcp_with_prelude, bridge_ws_tcp};
use crate::modules::socks::{handle_socks5_handshake, is_ipv6, SocksCommand};
use crate::modules::telegram::{
    extract_dc, ip_to_dc, is_telegram_ip, patch_init_dc, ws_domains, MtProtoMessageSplitter,
    TelegramInitInfo, TelegramIpEntry,
};
use crate::modules::ws::RawWsClient;
use backoff::{begin_ws_probe, clear_ws_probe, disable_ws_route, record_ws_failure, WsDecision};
use ipv6_cache::{learned_ipv6_entry, remember_ipv6_entry};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{debug, info, warn};

#[derive(Debug)]
enum InitReadError {
    Io(std::io::Error),
    Timeout,
}

#[derive(Debug)]
struct ResolvedTelegramInit {
    packet: Vec<u8>,
    info: TelegramInitInfo,
    patched: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnknownDcAction {
    Close,
    DirectPassthrough,
}

struct WsConnectAttempt {
    client: Option<RawWsClient>,
    all_404: bool,
}

pub async fn route_client(
    mut stream: TcpStream,
    peer: std::net::SocketAddr,
    cfg: Arc<RoutedConfig>,
) {
    let peer = peer.to_string();
    let command = match handle_socks5_handshake(&mut stream, cfg.socks_auth.as_ref()).await {
        Ok(cmd) => cmd,
        Err(err) => {
            warn!("invalid socks request from {}: {:?}", peer, err);
            return;
        }
    };

    let SocksCommand::Connect {
        target_host,
        target_port,
    } = command;

    let target_is_ipv6 = is_ipv6(&target_host);
    if !is_telegram_ip(&target_host) {
        passthrough_non_telegram(stream, &peer, &target_host, target_port).await;
        return;
    }

    let init_packet = match read_telegram_init(&mut stream, cfg.read_timeout_secs).await {
        Ok(packet) => packet,
        Err(InitReadError::Io(err)) => {
            warn!("failed to read telegram init from {}: {}", peer, err);
            return;
        }
        Err(InitReadError::Timeout) => {
            warn!("telegram init timeout {}", peer);
            return;
        }
    };

    let resolved = resolve_telegram_init(&target_host, init_packet);
    remember_telegram_ipv6(&target_host, target_is_ipv6, &resolved.info);

    if is_http_transport(&resolved.packet) {
        debug!(
            "http transport rejected {} -> {}:{}",
            peer, target_host, target_port
        );
        return;
    }

    let Some(dc) = resolved.info.dc else {
        warn!("unknown dc {} -> {}:{}", peer, target_host, target_port);
        handle_unknown_dc(
            stream,
            &peer,
            &target_host,
            target_port,
            target_is_ipv6,
            &resolved.packet,
        )
        .await;
        return;
    };

    let ws_key = (dc, resolved.info.is_media);
    let now = Instant::now();
    match begin_ws_probe(ws_key, now) {
        WsDecision::ProbeNow => {}
        WsDecision::ProbeInFlight => {
            debug!(
                "ws probe already in flight dc={} media={} -> concurrent ws attempt {}",
                dc, resolved.info.is_media, peer
            );
        }
        WsDecision::Cooldown => {
            warn!(
                "ws cooldown active dc={} media={} -> closing {}",
                dc, resolved.info.is_media, peer
            );
            return;
        }
        WsDecision::Disabled404 => {
            warn!(
                "ws disabled after 404 dc={} media={} -> closing {}",
                dc, resolved.info.is_media, peer
            );
            return;
        }
    }

    let ws_attempt = connect_ws_route(&peer, cfg.as_ref(), dc, resolved.info.is_media).await;
    if let Some(ws) = ws_attempt.client {
        clear_ws_probe(ws_key);
        let splitter = MtProtoMessageSplitter::new(&resolved.packet);
        let session = format!(
            "peer={} dc={} media={} target={} via_ws",
            peer, dc, resolved.info.is_media, target_host
        );
        if let Err(err) = bridge_ws_tcp(stream, ws, resolved.packet, splitter, &session).await {
            warn!("ws bridge err {}: {}", peer, err);
        }
        return;
    }

    if ws_attempt.all_404 {
        disable_ws_route(ws_key);
        warn!(
            "ws disabled for runtime after HTTP 404 dc={} media={}",
            dc, resolved.info.is_media
        );
    } else {
        record_ws_failure(ws_key, now);
    }

    if resolved.patched {
        debug!("ws failed after patched init {} from {}", peer, target_host);
    }
    warn!(
        "ws route unavailable dc={} media={} peer={}",
        dc, resolved.info.is_media, peer
    );
}

fn is_http_transport(buf: &[u8]) -> bool {
    buf.starts_with(b"POST ")
        || buf.starts_with(b"GET ")
        || buf.starts_with(b"HEAD ")
        || buf.starts_with(b"OPTIONS ")
}

pub fn route_decision(host: &str, port: u16, has_dc: bool) -> &'static str {
    if !is_telegram_ip(host) {
        return "tcp_passthrough";
    }
    if has_dc && port != 0 {
        "ws_only"
    } else {
        "unknown_dc"
    }
}

async fn passthrough_non_telegram(
    stream: TcpStream,
    peer: &str,
    target_host: &str,
    target_port: u16,
) {
    debug!("passthrough {} -> {}:{}", peer, target_host, target_port);
    if let Err(err) = bridge_tcp_tcp(stream, target_host, target_port).await {
        warn!("passthrough failed: {}", err);
    }
}

async fn read_telegram_init(
    stream: &mut TcpStream,
    timeout_secs: u64,
) -> Result<Vec<u8>, InitReadError> {
    let mut init = [0u8; 64];
    match timeout(
        Duration::from_secs(timeout_secs),
        stream.read_exact(&mut init),
    )
    .await
    {
        Ok(Ok(_)) => Ok(init.to_vec()),
        Ok(Err(err)) => Err(InitReadError::Io(err)),
        Err(_) => Err(InitReadError::Timeout),
    }
}

fn resolve_telegram_init(target_host: &str, init_packet: Vec<u8>) -> ResolvedTelegramInit {
    let mut packet = init_packet;
    let mut info = extract_dc(&packet);
    let mut patched = false;

    if info.dc.is_none() {
        if let Some(entry) = learned_ipv6_entry(target_host).or_else(|| ip_to_dc(target_host)) {
            let signed_dc = if entry.is_media {
                -(entry.dc as i16)
            } else {
                entry.dc as i16
            };
            packet = patch_init_dc(&packet, signed_dc);
            info = extract_dc(&packet);
            patched = true;
            info.dc = Some(entry.dc);
            info.is_media = entry.is_media;
        }
    }

    ResolvedTelegramInit {
        packet,
        info,
        patched,
    }
}

fn remember_telegram_ipv6(target_host: &str, target_is_ipv6: bool, info: &TelegramInitInfo) {
    if target_is_ipv6 {
        if let Some(dc) = info.dc {
            remember_ipv6_entry(
                target_host,
                TelegramIpEntry {
                    dc,
                    is_media: info.is_media,
                },
            );
        }
    }
}

async fn handle_unknown_dc(
    stream: TcpStream,
    peer: &str,
    target_host: &str,
    target_port: u16,
    target_is_ipv6: bool,
    init_packet: &[u8],
) {
    if unknown_dc_action(target_is_ipv6) == UnknownDcAction::DirectPassthrough {
        debug!(
            "unknown dc for ipv6 target, direct passthrough {} -> {}:{}",
            peer, target_host, target_port
        );
        let _ = bridge_tcp_tcp_with_prelude(stream, target_host, target_port, init_packet).await;
    }
}

fn unknown_dc_action(target_is_ipv6: bool) -> UnknownDcAction {
    if target_is_ipv6 {
        UnknownDcAction::DirectPassthrough
    } else {
        UnknownDcAction::Close
    }
}

async fn connect_ws_route(
    peer: &str,
    cfg: &RoutedConfig,
    dc: u8,
    is_media: bool,
) -> WsConnectAttempt {
    let mut ws_client = None;
    let mut all_404 = true;

    for domain in ws_domains(dc, is_media) {
        match RawWsClient::connect(&domain, Duration::from_secs(cfg.connect_timeout_secs)).await {
            Ok(ws) => {
                info!(
                    "ws route selected dc={} media={} -> {}",
                    dc, is_media, domain
                );
                ws_client = Some(ws);
                break;
            }
            Err(err) => {
                if err.status_code() != Some(404) {
                    all_404 = false;
                }
                warn!("ws connect failed {} -> {} ({})", peer, domain, err);
            }
        }
    }

    WsConnectAttempt {
        client: ws_client,
        all_404,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        remember_ipv6_entry, resolve_telegram_init, route_decision, unknown_dc_action,
        UnknownDcAction,
    };
    use aes::cipher::{KeyIvInit, StreamCipher};
    use aes::Aes256;

    type Aes256Ctr = ctr::Ctr128BE<Aes256>;

    fn build_init(dc: i16) -> Vec<u8> {
        let mut init = [0u8; 64];
        init[8..40].copy_from_slice(&[3u8; 32]);
        init[40..56].copy_from_slice(&[4u8; 16]);

        let mut cipher = Aes256Ctr::new_from_slices(&init[8..40], &init[40..56]).unwrap();
        let mut stream = [0u8; 64];
        cipher.apply_keystream(&mut stream);

        let mut plain = [0u8; 8];
        plain[0..4].copy_from_slice(&0xEFEFEFEFu32.to_le_bytes());
        plain[4..6].copy_from_slice(&dc.to_le_bytes());
        for idx in 0..8 {
            init[56 + idx] = stream[56 + idx] ^ plain[idx];
        }

        init.to_vec()
    }

    #[test]
    fn route_rules() {
        assert_eq!(route_decision("8.8.8.8", 443, false), "tcp_passthrough");
        assert_eq!(route_decision("149.154.175.50", 443, true), "ws_only");
        assert_eq!(route_decision("2001:67c:4e8::1", 443, true), "ws_only");
        assert_eq!(route_decision("2001:db8::1", 443, true), "tcp_passthrough");
        assert_eq!(route_decision("149.154.175.50", 443, false), "unknown_dc");
        assert_eq!(route_decision("2001:67c:4e8::1", 443, false), "unknown_dc");
    }

    #[test]
    fn resolve_telegram_init_patches_from_static_ipv4_mapping() {
        let resolved = resolve_telegram_init("149.154.167.220", build_init(0));
        assert_eq!(resolved.info.dc, Some(2));
        assert!(!resolved.info.is_media);
        assert!(resolved.patched);
    }

    #[test]
    fn resolve_telegram_init_uses_learned_ipv6_mapping() {
        let host = "2001:67c:4e8::1234";
        remember_ipv6_entry(
            host,
            crate::modules::telegram::TelegramIpEntry {
                dc: 4,
                is_media: true,
            },
        );

        let resolved = resolve_telegram_init(host, build_init(0));
        assert_eq!(resolved.info.dc, Some(4));
        assert!(resolved.info.is_media);
        assert!(resolved.patched);
    }

    #[test]
    fn unknown_dc_for_ipv6_falls_back_to_passthrough() {
        assert_eq!(unknown_dc_action(true), UnknownDcAction::DirectPassthrough);
        assert_eq!(unknown_dc_action(false), UnknownDcAction::Close);
    }
}
