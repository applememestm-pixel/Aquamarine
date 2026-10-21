# Release image: packs the aqua-server binaries that release.yml already
# built (linux x64 / arm64) — no compile here. Same layout as ../../Dockerfile.
FROM debian:bookworm-slim
ARG TARGETARCH
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data aqua \
 && mkdir -p /data && chown aqua /data
COPY bin/aqua-server-${TARGETARCH} /usr/local/bin/aqua-server
RUN chmod 755 /usr/local/bin/aqua-server
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
      org.opencontainers.image.description="Relay server of the Aquamarine end-to-end encrypted messenger" \
      org.opencontainers.image.source="https://github.com/applememestm-pixel/Aquamarine" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
