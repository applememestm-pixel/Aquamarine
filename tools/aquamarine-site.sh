#!/usr/bin/env bash
# Сайт Aquamarine (страницы + документация) — где угодно: Arch, macOS,
# Termux на телефоне. Ретранслятор тут ни при чём, он запускается отдельно
# (aquamarine-server.sh).
#
#   bash aquamarine-site.sh            # отдать сайт на http://<адрес>:8080/
#   bash aquamarine-site.sh update     # скачать свежую версию и отдать
#   bash aquamarine-site.sh docker     # то же в Docker-контейнере (фоном, с автозапуском)
#   bash aquamarine-site.sh stop       # остановить контейнер
#   PORT=9000 bash aquamarine-site.sh  # другой порт
#   TAG=latest bash aquamarine-site.sh update   # стабильный релиз вместо nightly
#
# Termux: pkg install curl python   (скрипт сам предложит), затем
#   curl -fsSLO https://raw.githubusercontent.com/applememestm-pixel/Aquamarine/main/tools/aquamarine-site.sh
#   bash aquamarine-site.sh
set -euo pipefail

REPO="applememestm-pixel/Aquamarine"
PORT="${PORT:-8080}"
TAG="${TAG:-nightly}"
IMAGE="ghcr.io/applememestm-pixel/aquamarine-site:latest"
NAME="aquamarine-site"
HERE="$(cd "$(dirname "$(readlink -f "$0" 2>/dev/null || echo "$0")")" && pwd)"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}/aquamarine-site"
WWW="$DATA/www"
CMD="${1:-serve}"
TERMUX=0; case "${PREFIX:-}" in *com.termux*) TERMUX=1;; esac

say() { printf '%s\n' "$*"; }
die() { printf '[X] %s\n' "$*" >&2; exit 1; }

lan_ip() {
  local ip=""
  if command -v ip >/dev/null 2>&1; then
    ip=$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<NF;i++) if($i=="src") print $(i+1)}' || true)
  fi
  if [ -z "$ip" ] && command -v python3 >/dev/null 2>&1; then
    ip=$(python3 -c 'import socket;s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.connect(("1.1.1.1",80));print(s.getsockname()[0])' 2>/dev/null || true)
  fi
  if [ -z "$ip" ] && command -v ifconfig >/dev/null 2>&1; then
    ip=$(ifconfig 2>/dev/null | awk '/inet /{print $2}' | grep -v '^127\.' | head -n1 || true)
  fi
  printf '%s' "$ip"
}

addresses() {
  local ip; ip=$(lan_ip)
  say ""
  say "Сайт Aquamarine:"
  say "  на этом устройстве:  http://127.0.0.1:$PORT/"
  [ -n "$ip" ] && say "  в локальной сети:    http://$ip:$PORT/"
  say "  в интернете всегда:  https://applememestm-pixel.github.io/Aquamarine/"
  say ""
}

# --- Docker -----------------------------------------------------------------
docker_cmd() {
  command -v docker >/dev/null 2>&1 || die "Docker не установлен. Без него: bash $0"
  if command docker info >/dev/null 2>&1; then DOCKER=(docker); else
    say "Нет доступа к Docker без sudo — запускаю через sudo."
    DOCKER=(sudo docker)
  fi
}

case "$CMD" in
  docker)
    docker_cmd
    "${DOCKER[@]}" pull "$IMAGE"
    "${DOCKER[@]}" rm -f "$NAME" >/dev/null 2>&1 || true
    "${DOCKER[@]}" run -d --name "$NAME" --restart unless-stopped -p "$PORT:8080" "$IMAGE" >/dev/null
    addresses
    say "Контейнер $NAME работает фоном и поднимается сам после перезагрузки."
    say "Остановить: bash $0 stop"
    exit 0 ;;
  stop)
    docker_cmd
    "${DOCKER[@]}" rm -f "$NAME" >/dev/null 2>&1 && say "Контейнер $NAME остановлен." || say "Контейнера $NAME нет."
    exit 0 ;;
  serve|update) ;;
  -h|--help|help) sed -n '2,15p' "$0"; exit 0 ;;
  *) die "неизвестная команда: $CMD (serve | update | docker | stop)" ;;
esac

# --- Files ------------------------------------------------------------------
need() {
  command -v "$1" >/dev/null 2>&1 && return 0
  if [ "$TERMUX" = 1 ]; then
    say "Ставлю $2 (pkg install $2)…"
    pkg install -y "$2" >/dev/null || die "не удалось поставить $2: pkg install $2"
  else
    die "нужен $1 — поставь пакет $2 (Arch: sudo pacman -S $2)"
  fi
}

# From a checkout (site/ + docs/ next to tools/) or a folder that already has the site.
local_source() {
  local d
  for d in "${SITE_SRC:-}" "$HERE/site" "$HERE/../site"; do
    [ -n "$d" ] && [ -f "$d/index.html" ] && { (cd "$d" && pwd); return 0; }
  done
  return 1
}

fetch_site() {
  need curl curl
  need tar tar
  local tmp; tmp=$(mktemp -d)
  local url="https://github.com/$REPO/releases/download/$TAG/aquamarine-site.tar.gz"
  [ "$TAG" = latest ] && url="https://github.com/$REPO/releases/latest/download/aquamarine-site.tar.gz"
  say "Скачиваю сайт ($TAG)…"
  if curl -fsSL --retry 2 "$url" -o "$tmp/site.tgz" && tar -xzf "$tmp/site.tgz" -C "$tmp" && [ -f "$tmp/aquamarine-site/index.html" ]; then
    :
  else
    # No bundle in the release yet — take the pages straight from the repository.
    say "Архива в релизе нет — беру из репозитория (main)…"
    curl -fsSL --retry 2 "https://codeload.github.com/$REPO/tar.gz/refs/heads/main" -o "$tmp/repo.tgz" \
      || die "не удалось скачать сайт — есть ли интернет?"
    tar -xzf "$tmp/repo.tgz" -C "$tmp"
    local src; src=$(find "$tmp" -maxdepth 1 -type d -name 'Aquamarine-*' | head -n1)
    [ -f "$src/site/index.html" ] || die "в архиве нет site/index.html"
    mkdir -p "$tmp/aquamarine-site/docs"
    cp -r "$src/site/." "$tmp/aquamarine-site/"
    cp "$src"/docs/*.md "$tmp/aquamarine-site/docs/" 2>/dev/null || true
    cp "$src/README.md" "$tmp/aquamarine-site/docs/passport.md" 2>/dev/null || true
  fi
  rm -rf "$tmp/aquamarine-site/downloads"
  mkdir -p "$DATA"
  rm -rf "$WWW.new" && mv "$tmp/aquamarine-site" "$WWW.new"
  rm -rf "$WWW" && mv "$WWW.new" "$WWW"
  rm -rf "$tmp"
}

if src=$(local_source); then
  # A checkout: lay the pages and the Markdown documents out side by side.
  docs="$src/../docs"
  if [ -d "$docs" ] && [ ! -d "$src/docs" ]; then
    mkdir -p "$DATA"; rm -rf "$WWW"; mkdir -p "$WWW/docs"
    cp -r "$src/." "$WWW/"; rm -rf "$WWW/downloads"
    cp "$docs"/*.md "$WWW/docs/" 2>/dev/null || true
    [ -f "$src/../README.md" ] && cp "$src/../README.md" "$WWW/docs/passport.md"
    # Local builds (build-all.sh) next to the site → download buttons use them.
    [ -d "$src/downloads" ] && ln -s "$src/downloads" "$WWW/downloads" 2>/dev/null || true
  else
    WWW="$src"
  fi
  say "Сайт из папки: $src"
elif [ "$CMD" = update ] || [ ! -f "$WWW/index.html" ]; then
  fetch_site
fi

# --- Serve ------------------------------------------------------------------
[ "$TERMUX" = 1 ] && command -v termux-wake-lock >/dev/null 2>&1 && termux-wake-lock 2>/dev/null || true
addresses
say "Ctrl+C — остановить."
say ""

if command -v python3 >/dev/null 2>&1 || { [ "$TERMUX" = 1 ] && need python3 python; }; then
  cd "$WWW"
  exec python3 - "$PORT" <<'PY'
import http.server, socketserver, sys, mimetypes
mimetypes.add_type("text/markdown; charset=utf-8", ".md")
mimetypes.add_type("text/javascript; charset=utf-8", ".js")
class H(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()
    def log_message(self, fmt, *args):
        sys.stdout.write("%s  %s\n" % (self.address_string(), fmt % args)); sys.stdout.flush()
class S(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True
try:
    S(("0.0.0.0", int(sys.argv[1])), H).serve_forever()
except OSError as e:
    sys.exit("[X] порт %s занят (%s). Другой порт: PORT=9000 bash aquamarine-site.sh" % (sys.argv[1], e.strerror))
except KeyboardInterrupt:
    pass
PY
elif command -v busybox >/dev/null 2>&1; then
  exec busybox httpd -f -v -p "$PORT" -h "$WWW"
elif command -v php >/dev/null 2>&1; then
  exec php -S "0.0.0.0:$PORT" -t "$WWW"
else
  die "нужен python3 (Arch: sudo pacman -S python; Termux: pkg install python)"
fi
