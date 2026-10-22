# Aquamarine website (site/ + the Markdown documents it renders) — a tiny
# static image, ~2 MB, for amd64 / arm64 / armv7. The relay is NOT in here:
# it runs natively (aquamarine-server.sh).
#
#   docker run -d --name aquamarine-site --restart unless-stopped -p 8080:8080 \
#     ghcr.io/applememestm-pixel/aquamarine-site
#   docker compose up -d        # the same, built from this checkout
#
# Without Docker (Termux on a phone, any Linux/macOS): tools/aquamarine-site.sh

FROM busybox:1.36-musl
COPY site /srv/site
COPY docs/*.md /srv/site/docs/
COPY README.md /srv/site/docs/passport.md
RUN rm -rf /srv/site/downloads \
 && printf '%s\n' \
      '.html:text/html; charset=utf-8' \
      '.css:text/css; charset=utf-8' \
      '.js:text/javascript; charset=utf-8' \
      '.md:text/markdown; charset=utf-8' \
      '.svg:image/svg+xml' \
      '.png:image/png' \
      '.ico:image/x-icon' \
      '.webp:image/webp' > /etc/httpd.conf
USER 65534
EXPOSE 8080
HEALTHCHECK --interval=60s --timeout=5s CMD ["wget", "-q", "-O", "/dev/null", "http://127.0.0.1:8080/"]
CMD ["httpd", "-f", "-v", "-p", "8080", "-h", "/srv/site", "-c", "/etc/httpd.conf"]
LABEL org.opencontainers.image.title="Aquamarine website" \
      org.opencontainers.image.description="Website and documentation of the Aquamarine messenger" \
      org.opencontainers.image.source="https://github.com/applememestm-pixel/Aquamarine" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
