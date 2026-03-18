# Architecture

## Overview

`tg-ws-proxy-rs` is a Linux-first SOCKS5 proxy for Telegram clients.

The current design has one target behavior:

- Telegram IPv4 MTProto traffic is bridged through Telegram WebSocket endpoints `kws*.web.telegram.org`.
- Non-Telegram traffic is passed through as plain TCP.
- Telegram IPv6 traffic uses the same WS path when `dc/media` is present in init or learned in runtime cache; unknown first-hit IPv6 targets may still fall back to direct passthrough.

## Runtime Model

Process entrypoint:

- [main.rs](/home/evmenenko/Documents/gbunker/tproxy/src/main.rs)

Main responsibilities:

- parse CLI config
- initialize logging
- print SOCKS proxy link for Telegram
- start the SOCKS listener
- start the health endpoint
- wait for shutdown signal

## Main Components

### Configuration

- [config.rs](/home/evmenenko/Documents/gbunker/tproxy/src/config.rs)

Responsibilities:

- CLI parsing
- health endpoint address
- timeouts
- explicit announce addrs for printed proxy links
- `tg://socks?...` link generation

### TCP Server

- [server.rs](/home/evmenenko/Documents/gbunker/tproxy/src/runtime/server.rs)

Responsibilities:

- bind SOCKS listeners on both families for wildcard host
- accept inbound TCP connections
- spawn one async task per client
- spawn health endpoint server

### SOCKS5 Handshake

- [socks.rs](/home/evmenenko/Documents/gbunker/tproxy/src/modules/socks.rs)

Responsibilities:

- parse SOCKS5 greeting and CONNECT request
- support IPv4, domain and IPv6 target parsing
- return target host and port to routing layer

The proxy is currently unauthenticated.

### Routing

- [routing.rs](/home/evmenenko/Documents/gbunker/tproxy/src/modules/routing.rs)

Responsibilities:

- distinguish Telegram IPs from non-Telegram traffic
- read the first 64-byte MTProto obfuscation init packet
- extract or patch Telegram DC information
- select WebSocket domain `kws{dc}` or `kws{dc}-1`
- maintain short-lived WS backoff state for repeated failures
- decide whether traffic goes to:
  - direct TCP passthrough
  - WebSocket bridge
  - direct passthrough for unsupported IPv6/unknown-DC cases

Important behavior:

- Telegram IPv4 without a known DC is not rerouted to plain TCP.
- WebSocket `404` results are tracked and can temporarily disable repeated attempts for the same `(dc, media)` key.

### Telegram-Specific Logic

- [telegram.rs](/home/evmenenko/Documents/gbunker/tproxy/src/modules/telegram.rs)

Responsibilities:

- known Telegram IPv4 and IPv6 range detection
- IP-to-DC mapping for common Telegram IPv4 DC targets
- runtime learning of IPv6 `target -> dc/media`
- MTProto init packet DC extraction
- MTProto init packet DC patching when needed
- stateful MTProto abridged message splitting for TCP-to-WS framing

The splitter mirrors the original Python reference closely enough to preserve MTProto message boundaries after obfuscation.

### WebSocket Transport

- [ws.rs](/home/evmenenko/Documents/gbunker/tproxy/src/modules/ws.rs)

Responsibilities:

- raw TCP connect
- TLS handshake with Telegram Web endpoints
- manual HTTP Upgrade request for `/apiws`
- raw WebSocket frame encoding and decoding
- ping/pong handling
- close frame parsing with code and reason

The handshake is browser-like on purpose, because Telegram WebSocket endpoints are sensitive to request shape.

### Bidirectional Relay

- [relay.rs](/home/evmenenko/Documents/gbunker/tproxy/src/modules/relay.rs)

Responsibilities:

- send initial MTProto packet into WS
- run client-to-WS and WS-to-client tasks concurrently
- split MTProto packets before sending them as WebSocket frames
- log session duration and transferred bytes

The relay is full duplex. Read and write halves of the WebSocket stream are separated to avoid a shared lock blocking both directions.

### Health Endpoint

- [health.rs](/home/evmenenko/Documents/gbunker/tproxy/src/modules/health.rs)

Responsibilities:

- bind simple HTTP endpoint
- return `200 OK`

Default health address is `[::]:8080`, which binds both `0.0.0.0:8080` and `[::]:8080`.

## Data Flow

1. Client connects to SOCKS5 listener.
2. Proxy parses SOCKS5 CONNECT request.
3. If target is not a known Telegram IPv4/IPv6 address, proxy uses direct TCP passthrough.
4. If target looks like Telegram, proxy reads the 64-byte MTProto init packet.
5. Proxy extracts DC and media flag from obfuscated init, or patches it from known/static mapping.
6. For IPv6 targets with extracted DC, proxy stores a runtime mapping for later sessions to the same target.
7. Proxy selects a WebSocket endpoint such as `kws2.web.telegram.org`.
8. Proxy performs TLS + HTTP Upgrade.
9. Proxy sends MTProto init packet through WebSocket.
10. Proxy relays traffic both directions until one side closes.

## Observability

Important log lines:

- startup configuration and SOCKS link
- selected WebSocket route
- WS connection failures
- per-session bridge summary:
  - duration
  - uploaded bytes
  - downloaded bytes
  - closing side and reason

This is the main operational debugging surface today. There is no `/metrics` endpoint yet.

## Current Limitations

- No SOCKS5 authentication
- No metric backend
- No WebSocket pooling in active use
- Telegram IPv6 still depends on init extraction or runtime-learned mapping; fully static IPv6-to-DC coverage is not complete
- Some Telegram clients create many short-lived probe connections; this is expected noise in logs

## Container Layout

- [Dockerfile](/home/evmenenko/Documents/gbunker/tproxy/Dockerfile)

Build model:

- stage 1: compile release binary with Rust toolchain
- stage 2: run on `rust:1-slim`
- current release flow is temporarily focused on `linux/amd64`
- exposes:
  - `1080/tcp` SOCKS5
  - `8080/tcp` health

## Design Summary

The system is intentionally small:

- one listener
- one routing decision layer
- one Telegram-specific protocol adapter
- one raw WebSocket transport
- one duplex relay

That keeps debugging tractable while the Telegram WebSocket behavior is still being reverse engineered and stabilized.
