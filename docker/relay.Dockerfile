# Aquamarine relay (aqua-server), built from source inside Docker — nothing
# to install but Docker itself, no clone needed:
#
#   docker build -t aquamarine-server -f docker/relay.Dockerfile \
#     https://github.com/applememestm-pixel/Aquamarine.git#main
#
# The published image (ghcr.io/applememestm-pixel/aquamarine-server) is made
# from the release binaries instead (docker/relay-bin.Dockerfile) — use it
# unless you changed the code. How to run either: docs/docker.md.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p aqua-server --locked \
 && cp target/release/aqua-server /aqua-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data aqua && mkdir -p /data && chown aqua /data
COPY --from=build /aqua-server /usr/local/bin/aqua-server
USER aqua
WORKDIR /data
VOLUME /data
ENV AQUA_DATA=/data AQUA_LISTEN=0.0.0.0:7777 AQUA_DATA_PROMPT=0 AQUA_TG_PROMPT=0
EXPOSE 7777
HEALTHCHECK --interval=60s --timeout=5s CMD ["wget", "-q", "-O", "/dev/null", "http://127.0.0.1:7777/v1/stats"]
CMD ["aqua-server"]
LABEL org.opencontainers.image.title="Aquamarine relay" \
      org.opencontainers.image.source="https://github.com/applememestm-pixel/Aquamarine" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
