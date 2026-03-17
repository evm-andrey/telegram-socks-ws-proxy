# Changelog

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
