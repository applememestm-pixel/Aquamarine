#!/usr/bin/env bash
# Starts (or updates) the Aquamarine relay in Docker and follows its log.
#   bash aquamarine-docker.sh            # image :nightly (последний коммит main)
#   TAG=latest bash aquamarine-docker.sh # последний стабильный релиз
#   PORT=8080 bash aquamarine-docker.sh  # другой порт на хосте
set -euo pipefail

IMAGE="ghcr.io/applememestm-pixel/aquamarine-server:${TAG:-nightly}"
NAME="${NAME:-aquamarine}"
PORT="${PORT:-7777}"

if ! docker info >/dev/null 2>&1; then
  echo "Docker недоступен. Запустите службу и добавьте себя в группу docker:"
  echo "  sudo systemctl enable --now docker && sudo usermod -aG docker \$USER && newgrp docker"
  exit 1
fi

echo "→ $IMAGE"
docker pull "$IMAGE"
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" --restart unless-stopped \
  -p "$PORT:7777" -v aquamarine-data:/data \
  ${AQUA_ADMIN_TOKEN:+-e AQUA_ADMIN_TOKEN="$AQUA_ADMIN_TOKEN"} \
  "$IMAGE" >/dev/null

ip=$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<NF;i++) if($i=="src") print $(i+1)}')
echo
echo "Ретранслятор запущен. Адрес для клиента:"
echo "  на этом компьютере:  http://127.0.0.1:$PORT"
[ -n "$ip" ] && echo "  в локальной сети:    http://$ip:$PORT"
echo "Данные — в томе aquamarine-data. Остановить: docker stop $NAME"
echo "Журнал (Ctrl+C — выйти, сервер продолжит работать):"
echo
exec docker logs -f "$NAME"
