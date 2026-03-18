use crate::config::RoutedConfig;
use crate::modules::relay::{bridge_tcp_tcp, bridge_tcp_tcp_with_prelude, bridge_ws_tcp};
use crate::modules::socks::{handle_socks5_handshake, is_ipv6, SocksCommand};
use crate::modules::telegram::{
    extract_dc, ip_to_dc, is_telegram_ip, patch_init_dc, ws_domains, MtProtoMessageSplitter,
    TelegramIpEntry,
};
use crate::modules::ws::RawWsClient;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{debug, info, warn};

const WS_FAILURE_COOLDOWN: Duration = Duration::from_secs(60);
const WS_ATTEMPT_WINDOW: Duration = Duration::from_secs(15);

static WS_BACKOFF: Lazy<Mutex<WsBackoff>> = Lazy::new(|| Mutex::new(WsBackoff::default()));
static LEARNED_IPV6_DC: Lazy<Mutex<HashMap<Ipv6Addr, TelegramIpEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Default)]
struct WsBackoff {
    states: HashMap<(u8, bool), WsState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WsState {
    Probing(Instant),
    Cooldown(Instant),
    Disabled404,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WsDecision {
    ProbeNow,
    ProbeInFlight,
    Cooldown,
    Disabled404,
}

impl WsBackoff {
    fn begin_probe(&mut self, key: (u8, bool), now: Instant) -> WsDecision {
        self.states.retain(|_, state| match state {
            WsState::Probing(until) | WsState::Cooldown(until) => *until > now,
            WsState::Disabled404 => true,
        });

        match self.states.get(&key).copied() {
            Some(WsState::Probing(_)) => WsDecision::ProbeInFlight,
            Some(WsState::Cooldown(_)) => WsDecision::Cooldown,
            Some(WsState::Disabled404) => WsDecision::Disabled404,
            None => {
                self.states
                    .insert(key, WsState::Probing(now + WS_ATTEMPT_WINDOW));
                WsDecision::ProbeNow
            }
        }
    }

    fn record_failure(&mut self, key: (u8, bool), now: Instant, cooldown: Duration) {
        self.states.insert(key, WsState::Cooldown(now + cooldown));
    }

    fn disable_404(&mut self, key: (u8, bool)) {
        self.states.insert(key, WsState::Disabled404);
    }

    fn clear(&mut self, key: (u8, bool)) {
        self.states.remove(&key);
    }
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
    let target_is_telegram = is_telegram_ip(&target_host);

    if !target_is_telegram {
        debug!("passthrough {} -> {}:{}", peer, target_host, target_port);
        if let Err(err) = bridge_tcp_tcp(stream, &target_host, target_port).await {
            warn!("passthrough failed: {}", err);
        }
        return;
    }

    let mut init = [0u8; 64];
    match timeout(
        Duration::from_secs(cfg.read_timeout_secs),
        stream.read_exact(&mut init),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(err)) => {
            warn!("failed to read telegram init from {}: {}", peer, err);
            return;
        }
        Err(_) => {
            warn!("telegram init timeout {}", peer);
            return;
        }
    }

    let mut init_packet = init.to_vec();
    let mut info = extract_dc(&init_packet);
    let mut patched = false;

    if info.dc.is_none() {
        if let Some(entry) = learned_ipv6_entry(&target_host).or_else(|| ip_to_dc(&target_host)) {
            let dc_signed = if entry.is_media {
                -(entry.dc as i16)
            } else {
                entry.dc as i16
            };
            let patched_init = patch_init_dc(&init_packet, dc_signed);
            init_packet = patched_init;
            info = extract_dc(&init_packet);
            patched = true;
            info.dc = Some(entry.dc);
            info.is_media = entry.is_media;
        }
    }

    if target_is_ipv6 {
        if let Some(dc) = info.dc {
            remember_ipv6_entry(
                &target_host,
                TelegramIpEntry {
                    dc,
                    is_media: info.is_media,
                },
            );
        }
    }

    if is_http_transport(&init_packet) {
        debug!(
            "http transport rejected {} -> {}:{}",
            peer, target_host, target_port
        );
        return;
    }

    let Some(dc) = info.dc else {
        warn!("unknown dc {} -> {}:{}", peer, target_host, target_port);
        if target_is_ipv6 {
            debug!(
                "unknown dc for ipv6 target, direct passthrough {} -> {}:{}",
                peer, target_host, target_port
            );
            let _ =
                bridge_tcp_tcp_with_prelude(stream, &target_host, target_port, &init_packet).await;
            return;
        }
        return;
    };

    let domains = ws_domains(dc, info.is_media);
    let ws_key = (dc, info.is_media);
    let now = Instant::now();
    let decision = WS_BACKOFF
        .lock()
        .expect("ws backoff lock")
        .begin_probe(ws_key, now);

    match decision {
        WsDecision::ProbeNow => {}
        WsDecision::ProbeInFlight => {
            debug!(
                "ws probe already in flight dc={} media={} -> concurrent ws attempt {}",
                dc, info.is_media, peer
            );
        }
        WsDecision::Cooldown => {
            warn!(
                "ws cooldown active dc={} media={} -> closing {}",
                dc, info.is_media, peer
            );
            return;
        }
        WsDecision::Disabled404 => {
            warn!(
                "ws disabled after 404 dc={} media={} -> closing {}",
                dc, info.is_media, peer
            );
            return;
        }
    }

    let mut ws_client = None;
    let mut all_404 = true;
    for domain in domains {
        match RawWsClient::connect(&domain, Duration::from_secs(cfg.connect_timeout_secs)).await {
            Ok(ws) => {
                info!(
                    "ws route selected dc={} media={} -> {}",
                    dc, info.is_media, domain
                );
                ws_client = Some(ws);
                break;
            }
            Err(err) => {
                if err.status_code() != Some(404) {
                    all_404 = false;
                }
                warn!("ws connect failed {} -> {} ({})", peer, domain, err);
                continue;
            }
        }
    }

    if let Some(ws) = ws_client {
        WS_BACKOFF.lock().expect("ws backoff lock").clear(ws_key);
        let splitter = MtProtoMessageSplitter::new(&init_packet);
        let session = format!(
            "peer={} dc={} media={} target={} via_ws",
            peer, dc, info.is_media, target_host
        );
        if let Err(err) = bridge_ws_tcp(stream, ws, init_packet, splitter, &session).await {
            warn!("ws bridge err {}: {}", peer, err);
        }
        return;
    }

    if all_404 {
        WS_BACKOFF
            .lock()
            .expect("ws backoff lock")
            .disable_404(ws_key);
        warn!(
            "ws disabled for runtime after HTTP 404 dc={} media={}",
            dc, info.is_media
        );
    } else {
        WS_BACKOFF.lock().expect("ws backoff lock").record_failure(
            ws_key,
            now,
            WS_FAILURE_COOLDOWN,
        );
    }

    if patched {
        debug!("ws failed after patched init {} from {}", peer, target_host);
    }
    warn!(
        "ws route unavailable dc={} media={} peer={}",
        dc, info.is_media, peer
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

fn learned_ipv6_entry(host: &str) -> Option<TelegramIpEntry> {
    let ip = host.parse::<Ipv6Addr>().ok()?;
    LEARNED_IPV6_DC
        .lock()
        .expect("ipv6 dc cache lock")
        .get(&ip)
        .cloned()
}

fn remember_ipv6_entry(host: &str, entry: TelegramIpEntry) {
    let Ok(ip) = host.parse::<Ipv6Addr>() else {
        return;
    };
    LEARNED_IPV6_DC
        .lock()
        .expect("ipv6 dc cache lock")
        .insert(ip, entry);
}

#[cfg(test)]
mod tests {
    use super::{route_decision, WsBackoff, WsDecision};
    use std::time::{Duration, Instant};

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
    fn ws_backoff_expires_after_cooldown() {
        let mut backoff = WsBackoff::default();
        let key = (2, false);
        let now = Instant::now();

        assert_eq!(backoff.begin_probe(key, now), WsDecision::ProbeNow);
        backoff.record_failure(key, now, Duration::from_secs(5));
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(4)),
            WsDecision::Cooldown
        );
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(5)),
            WsDecision::ProbeNow
        );
    }

    #[test]
    fn ws_backoff_can_be_cleared() {
        let mut backoff = WsBackoff::default();
        let key = (4, true);
        let now = Instant::now();

        backoff.record_failure(key, now, Duration::from_secs(30));
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(1)),
            WsDecision::Cooldown
        );
        backoff.clear(key);
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(1)),
            WsDecision::ProbeNow
        );
    }

    #[test]
    fn ws_backoff_blocks_parallel_probe_window() {
        let mut backoff = WsBackoff::default();
        let key = (2, true);
        let now = Instant::now();

        assert_eq!(backoff.begin_probe(key, now), WsDecision::ProbeNow);
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(1)),
            WsDecision::ProbeInFlight
        );
    }

    #[test]
    fn ws_backoff_can_disable_404_for_runtime() {
        let mut backoff = WsBackoff::default();
        let key = (5, false);
        let now = Instant::now();

        backoff.disable_404(key);
        assert_eq!(backoff.begin_probe(key, now), WsDecision::Disabled404);
    }
}
