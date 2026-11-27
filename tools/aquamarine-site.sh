#!/usr/bin/env bash
# Сайт Aquamarine (страницы + документация) — где угодно: Arch, macOS,
# Termux на телефоне. Ретранслятор тут ни при чём, он запускается отдельно
# (aquamarine-server.sh).
#
#   bash aquamarine-site.sh            # отдать сайт на http://<адрес>:8080/
#   bash aquamarine-site.sh update     # скачать свежую версию и отдать
#   bash aquamarine-site.sh docker     # то же в Docker-контейнере (фоном, с автозапуском)
#   bash aquamarine-site.sh image server   # проверенный подписью образ сервера (по digest)
#   bash aquamarine-site.sh stop       # остановить контейнер
#   PORT=9000 bash aquamarine-site.sh  # другой порт
#   TAG=v1.21.0-24 bash aquamarine-site.sh update   # другой выпуск (по умолчанию — последний)
#   FROM_MAIN=1 bash aquamarine-site.sh update     # страницы из main, без проверки подписи
#
# Termux: pkg install curl python   (скрипт сам предложит), затем
#   curl -fsSLO https://raw.githubusercontent.com/applememestm-pixel/Aquamarine/main/tools/aquamarine-site.sh
#   bash aquamarine-site.sh
set -euo pipefail

REPO="applememestm-pixel/Aquamarine"
PORT="${PORT:-8080}"
# Ночных сборок больше нет: по умолчанию — последний выпуск.
TAG="${TAG:-latest}"
# Открытый ключ подписи выпусков (= переменная репозитория UPDATE_PUBKEY;
# release-signing-check.yml сверяет). Сайт ставится, только если архив
# назван в SHA256SUMS.txt, подписанном этим ключом (AQUA-SEC-012).
PUBKEY="d5753e7d1aa56046d29e0bbcf220497d78a4def55fec7c62d862e1d42f095d96"
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

# Hex → bytes (без xxd: его нет в Termux по умолчанию).
unhex() { printf '%b' "$(sed 's/../\\x&/g')"; }
sha256() { openssl dgst -sha256 -r "$1" | cut -d' ' -f1; }

# The release's SHA256SUMS.txt is signed by PUBKEY, names release $2 (its
# VERSION line) and lists file $3 with its hash. Anything else: stop.
verify_release() {
  local d=$1 version=${2#v} file=$3
  command -v openssl >/dev/null 2>&1 || die "для проверки подписи нужен openssl (Termux: pkg install openssl-tool; Debian: sudo apt install openssl)"
  [ -s "$d/SHA256SUMS.txt" ] && [ -s "$d/SHA256SUMS.txt.sig" ] || die "у выпуска $2 нет подписанного списка сумм — сайт не установлен (другой выпуск: TAG=v1.21.0-24 bash $0 update)"
  printf '302a300506032b6570032100%s' "$PUBKEY" | unhex > "$d/pub.der"
  openssl pkey -pubin -inform DER -in "$d/pub.der" -out "$d/pub.pem" 2>/dev/null || die "openssl не читает ключ Ed25519 — нужна версия 1.1.1 или новее"
  tr -d ' \n\r' < "$d/SHA256SUMS.txt.sig" | unhex > "$d/sig.bin"
  openssl pkeyutl -verify -pubin -inkey "$d/pub.pem" -rawin -in "$d/SHA256SUMS.txt" -sigfile "$d/sig.bin" >/dev/null 2>&1 \
    || die "подпись списка сумм выпуска $2 неверна — сайт не установлен"
  local want; want=$(printf '%s' "$version" > "$d/VERSION.want" && sha256 "$d/VERSION.want")
  grep -qiE "^$want [ *]?VERSION\$" "$d/SHA256SUMS.txt" || die "подписанный список сумм не от выпуска $2 — сайт не установлен"
  local h; h=$(sha256 "$d/$file")
  grep -qiE "^$h [ *]?$file\$" "$d/SHA256SUMS.txt" || die "$file не совпадает с подписанным списком сумм — сайт не установлен"
}

# The release tag to use: TAG, or the latest release when TAG=latest.
release_tag() {
  local tag=$TAG
  if [ "$tag" = latest ]; then
    # …/releases/latest ведёт на …/releases/tag/<тег>.
    tag=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") || die "не удалось узнать последний выпуск — есть ли интернет?"
    tag=${tag##*/tag/}
  fi
  case "$tag" in v[0-9]*) ;; *) die "не похоже на тег выпуска: $tag" ;; esac
  printf '%s' "$tag"
}

# The image named in a checked IMAGE-*.txt: exactly one reference by digest
# (AQUA-SEC-012: a tag such as :latest can be moved; a digest can't).
# $2 — which image was asked for (site | server): exactly that one.
image_ref() {
  local ref which=$2; ref=$(head -n1 "$1" | tr -d '\r')
  case "$which" in site|server) ;; *) die "образ: site или server" ;; esac
  printf '%s' "$ref" | grep -qE "^ghcr\\.io/applememestm-pixel/aquamarine-$which@sha256:[0-9a-f]{64}\$" || die "в $1 нет ссылки на образ $which по digest"
  printf '%s' "$ref"
}

# Download the release's IMAGE-<site|server>.txt with the signed sums,
# check them, print the image reference.
verified_image() {
  need curl curl
  local which=${1:-site} tmp tag; tmp=$(mktemp -d); tag=$(release_tag)
  case "$which" in site|server) ;; *) die "образ: site или server" ;; esac
  local file="IMAGE-$which.txt"
  for f in "$file" SHA256SUMS.txt SHA256SUMS.txt.sig; do
    curl -fsSL --retry 2 "https://github.com/$REPO/releases/download/$tag/$f" -o "$tmp/$f" \
      || [ "$f" != "$file" ] || die "у выпуска $tag нет подписанного digest образа $which"
  done
  verify_release "$tmp" "$tag" "$file"
  image_ref "$tmp/$file" "$which"
  rm -rf "$tmp"
}

fetch_site() {
  need curl curl
  need tar tar
  local tmp; tmp=$(mktemp -d)
  local tag; tag=$(release_tag)
  local base="https://github.com/$REPO/releases/download/$tag"
  if [ "${FROM_MAIN:-0}" = 1 ]; then
    # Явно попросили: страницы прямо из ветки main, БЕЗ проверки подписи.
    say "FROM_MAIN=1: беру страницы из ветки main без проверки подписи."
    curl -fsSL --retry 2 "https://codeload.github.com/$REPO/tar.gz/refs/heads/main" -o "$tmp/repo.tgz" \
      || die "не удалось скачать сайт — есть ли интернет?"
    tar -xzf "$tmp/repo.tgz" -C "$tmp"
    local src; src=$(find "$tmp" -maxdepth 1 -type d -name 'Aquamarine-*' | head -n1)
    [ -f "$src/site/index.html" ] || die "в архиве нет site/index.html"
    mkdir -p "$tmp/aquamarine-site/docs"
    cp -r "$src/site/." "$tmp/aquamarine-site/"
    cp "$src"/docs/*.md "$tmp/aquamarine-site/docs/" 2>/dev/null || true
    cp "$src/README.md" "$tmp/aquamarine-site/docs/passport.md" 2>/dev/null || true
  else
    say "Скачиваю сайт ($tag) и проверяю подпись…"
    for f in aquamarine-site.tar.gz SHA256SUMS.txt SHA256SUMS.txt.sig; do
      curl -fsSL --retry 2 "$base/$f" -o "$tmp/$f" || [ "$f" != aquamarine-site.tar.gz ] || die "в выпуске $tag нет архива сайта (страницы из main без проверки: FROM_MAIN=1 bash $0 update)"
    done
    verify_release "$tmp" "$tag" aquamarine-site.tar.gz
    tar -xzf "$tmp/aquamarine-site.tar.gz" -C "$tmp" && [ -f "$tmp/aquamarine-site/index.html" ] || die "архив сайта повреждён"
    say "Подпись верна: сайт выпуска $tag."
  fi
  rm -rf "$tmp/aquamarine-site/downloads"
  mkdir -p "$DATA"
  rm -rf "$WWW.new" && mv "$tmp/aquamarine-site" "$WWW.new"
  rm -rf "$WWW" && mv "$WWW.new" "$WWW"
  rm -rf "$tmp"
}

case "$CMD" in
  docker)
    docker_cmd
    # Only the image the signed release names, by its digest.
    IMAGE=$(verified_image)
    say "Образ (подпись выпуска верна): $IMAGE"
    "${DOCKER[@]}" pull "$IMAGE"
    "${DOCKER[@]}" rm -f "$NAME" >/dev/null 2>&1 || true
    "${DOCKER[@]}" run -d --name "$NAME" --restart unless-stopped -p "$PORT:8080" "$IMAGE" >/dev/null
    addresses
    say "Контейнер $NAME работает фоном и поднимается сам после перезагрузки."
    say "Остановить: bash $0 stop"
    exit 0 ;;
  image)
    # The signed reference by digest, for `docker run` / compose:
    #   docker run … "$(bash aquamarine-site.sh image server)"
    verified_image "${2:-server}"; echo
    exit 0 ;;
  stop)
    docker_cmd
    "${DOCKER[@]}" rm -f "$NAME" >/dev/null 2>&1 && say "Контейнер $NAME остановлен." || say "Контейнера $NAME нет."
    exit 0 ;;
  serve|update) ;;
  -h|--help|help) sed -n '2,15p' "$0"; exit 0 ;;
  *) die "неизвестная команда: $CMD (serve | update | docker | image | stop)" ;;
esac

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
