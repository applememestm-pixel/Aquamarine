# Aquamarine relay (aqua-server) — built from source.
#
#   docker compose up -d                      # with docker-compose.yml
#   docker build -t aquamarine-server .       # just the image
#
# Data (SQLite, attachments, packs, settings) lives in /data — mount a volume.

FROM rust:1-bookworm AS build
WORKDIR /src
# Only what the relay needs (the Tauri client is not part of the workspace).
COPY Cargo.toml Cargo.lock ./
COPY crates/aqua-core crates/aqua-core
COPY crates/aqua-client-core crates/aqua-client-core
COPY crates/aqua-server crates/aqua-server
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p aqua-server \
 && cp target/release/aqua-server /aqua-server \
 && strip /aqua-server

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data aqua \
 && mkdir -p /data && chown aqua /data
COPY --from=build /aqua-server /usr/local/bin/aqua-server
USER aqua
WORKDIR /data
ENV AQUA_LISTEN=0.0.0.0:7777 \
    AQUA_TG_PROMPT=0 \
    AQUA_IN_CONTAINER=1
EXPOSE 7777
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s \
  CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/${AQUA_LISTEN##*:} && printf 'GET /v1/stats HTTP/1.0\\r\\n\\r\\n' >&3 && grep -q '\"ok\":true' <&3"]
ENTRYPOINT ["/usr/bin/tini", "--", "aqua-server"]
LABEL org.opencontainers.image.title="Aquamarine relay" \
      org.opencontainers.image.source="https://github.com/applememestm-pixel/Aquamarine" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
