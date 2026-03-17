FROM rust:1 AS builder

WORKDIR /build
COPY . .
RUN cargo build --release

FROM rust:1-slim
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/tg-ws-proxy-rs /usr/local/bin/tg-ws-proxy-rs
RUN useradd -r -u 1000 appuser \
    && mkdir -p /config \
    && chown -R appuser:appuser /app /config
USER appuser
EXPOSE 1080 8080
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -sf http://127.0.0.1:8080/health >/dev/null || exit 1
ENTRYPOINT ["/usr/local/bin/tg-ws-proxy-rs"]
