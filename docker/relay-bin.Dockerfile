# Aquamarine relay image from the release binaries (release.yml, job
# "relay image"): docker/bin/amd64/aqua-server and docker/bin/arm64/aqua-server;
# the build context is the repository. To build from source instead: docker/relay.Dockerfile.
# How to run it: docs/docker.md.

FROM ubuntu:24.04
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data aqua && mkdir -p /data && chown aqua /data
COPY --chmod=755 docker/bin/${TARGETARCH}/aqua-server /usr/local/bin/aqua-server
USER aqua
WORKDIR /data
VOLUME /data
ENV AQUA_DATA=/data AQUA_LISTEN=0.0.0.0:7777 AQUA_DATA_PROMPT=0 AQUA_TG_PROMPT=0
EXPOSE 7777
HEALTHCHECK --interval=60s --timeout=5s CMD ["wget", "-q", "-O", "/dev/null", "http://127.0.0.1:7777/v1/stats"]
CMD ["aqua-server"]
LABEL org.opencontainers.image.title="Aquamarine relay" \
      org.opencontainers.image.description="Relay (server) of the Aquamarine end-to-end encrypted messenger" \
      org.opencontainers.image.source="https://github.com/applememestm-pixel/Aquamarine" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
