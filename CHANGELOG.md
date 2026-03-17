# Changelog

## 0.1.1 - 2026-03-17

- Switched TLS client stack from OpenSSL/native-tls to `rustls` with the `ring` backend.
- Reduced container image size and published a smaller multi-arch image for `linux/amd64` and `linux/arm64`.
- Verified deployment on RouterOS `7.22` (`RB5009UG+S+`) with a healthy container and reachable `1080`/`8080` ports.

## 2026-03-17

- Created Rust SOCKS5 proxy skeleton for Telegram WebSocket routing.
- Added Linux/container runtime with health endpoint and Docker image.
- Implemented Telegram-specific routing, DC extraction and DC patching.
- Implemented browser-like WebSocket handshake for `kws*.web.telegram.org/apiws`.
- Fixed critical relay issue by separating WebSocket read and write halves for full duplex.
- Added MTProto abridged message splitter for TCP-to-WS framing.
- Added proxy link generation in logs with `tg://socks?...`.
- Added session-level relay logs with duration and byte counters.
- Added parsing of WebSocket close frames with code and reason.
- Extended Telegram DC mapping, including `149.154.167.35`.
- Removed `auto` and `tcp` transport modes.
- Updated README and added architecture documentation.
- Added multi-arch container publishing flow for `linux/amd64` and `linux/arm64`.
