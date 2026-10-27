# Docker: свой сервер без сборки

> Часть [технической документации](README.md). Обычный запуск без Docker, TLS и переменные окружения — [deployment.md](deployment.md).

Ретранслятор (сервер Aquamarine) лежит готовым образом: `ghcr.io/applememestm-pixel/aquamarine-server`. Ничего собирать не нужно — нужен только сам Docker. Образ есть для обычных ПК и серверов (x86-64) и для ARM64 (Raspberry Pi 4/5 с 64-битной системой, Mac на Apple Silicon, ARM-серверы).

Коротко:

1. Поставить Docker (раздел 1).
2. Запустить одну команду (раздел 2).
3. В приложении ввести адрес `http://<IP этого компьютера>:7777`.

## 1. Поставить Docker

Проверка в конце одинаковая для всех систем:

```bash
docker version            # должны быть и Client, и Server
docker compose version
docker run --rm hello-world
```

### Windows 10 / 11

1. Откройте PowerShell **от имени администратора** и выполните `wsl --install`. Перезагрузите компьютер.
2. Скачайте **Docker Desktop** с [docker.com/products/docker-desktop](https://www.docker.com/products/docker-desktop/) и установите. Галочку «Use WSL 2» оставьте.
3. Запустите Docker Desktop и дождитесь зелёного «Engine running» внизу слева.
4. Команды из этой инструкции вводите в PowerShell или «Терминале» Windows.

Если Docker Desktop пишет «Virtualization not enabled» — включите виртуализацию в BIOS/UEFI: Intel VT-x, AMD SVM или «Virtualization Technology».

### macOS

- Скачайте **Docker Desktop** для своего процессора с [docker.com/products/docker-desktop](https://www.docker.com/products/docker-desktop/): Apple Silicon (M1–M4) или Intel. Перетащите его в «Программы» и запустите.
- Или через Homebrew: `brew install --cask docker`, затем запустите Docker из «Программ».
- Лёгкая альтернатива — [OrbStack](https://orbstack.dev/): команды те же.

### Ubuntu, Debian, Linux Mint, Raspberry Pi OS, Fedora, CentOS

Официальный скрипт сам определяет систему:

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker "$USER"   # чтобы не писать sudo перед каждой командой
```

После этого **выйдите из системы и войдите снова** (или перезагрузитесь). На Raspberry Pi нужна 64-битная Raspberry Pi OS: образа для 32-битной ARM нет.

### Arch Linux, Manjaro, EndeavourOS

```bash
sudo pacman -S docker docker-compose docker-buildx
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"   # затем выйти и войти снова
```

### Android (Termux)

Docker на телефоне не работает. Используйте встроенный сервер приложения: **Настройки → Сервер на устройстве**.

## 2. Запустить ретранслятор

### Одной командой

Придумайте длинный пароль для панели администратора и подставьте его вместо `ВАШ-ПАРОЛЬ-ДЛЯ-ПАНЕЛИ`.

Linux и macOS:

```bash
docker run -d --name aquamarine-server --restart unless-stopped \
  -p 7777:7777 -v aquamarine-data:/data \
  -e AQUA_ADMIN_TOKEN=ВАШ-ПАРОЛЬ-ДЛЯ-ПАНЕЛИ \
  ghcr.io/applememestm-pixel/aquamarine-server:latest
```

Windows (PowerShell), то же одной строкой:

```powershell
docker run -d --name aquamarine-server --restart unless-stopped -p 7777:7777 -v aquamarine-data:/data -e AQUA_ADMIN_TOKEN=ВАШ-ПАРОЛЬ-ДЛЯ-ПАНЕЛИ ghcr.io/applememestm-pixel/aquamarine-server:latest
```

Что здесь что:

| Часть | Зачем |
|---|---|
| `-d` | работает в фоне |
| `--restart unless-stopped` | сам поднимается после перезагрузки, пока вы его не остановили |
| `-p 7777:7777` | порт для приложений. Занят — пишите `-p 7778:7777` и подключайтесь к 7778 |
| `-v aquamarine-data:/data` | учётные записи, файлы и настройки живут в томе `aquamarine-data` и переживают обновления |
| `AQUA_ADMIN_TOKEN` | пароль панели. Без него при каждом запуске появляется новый — его видно в `docker logs aquamarine-server` |

### Через docker compose

Удобно, если хочется держать настройки в файле:

```bash
mkdir aquamarine && cd aquamarine
curl -fsSLO https://raw.githubusercontent.com/applememestm-pixel/Aquamarine/main/docker/compose.relay.yml
# откройте compose.relay.yml и впишите пароль в AQUA_ADMIN_TOKEN
docker compose -f compose.relay.yml up -d
```

В Windows вместо `curl -fsSLO` можно просто скачать файл по ссылке в браузере.

## 3. Подключиться

- **С этого же компьютера:** `http://127.0.0.1:7777`.
- **С телефона в той же Wi-Fi-сети:** `http://<IP компьютера>:7777`.
  - Windows: IP — в `ipconfig`, строка «IPv4-адрес».
  - macOS: `ipconfig getifaddr en0`.
  - Linux: `ip -4 addr` или `hostname -I`.
  - Панель внутри Docker может показать адрес вида `172.17.x.x` — это адрес контейнера, им не пользуйтесь.
- **Из интернета:** пробросьте порт 7777 на роутере на этот компьютер или поставьте перед сервером Caddy/nginx с HTTPS — [deployment.md, раздел 3](deployment.md#3-tls).

Панель администратора открывается в браузере на `http://127.0.0.1:7777/admin`, вход по паролю из `AQUA_ADMIN_TOKEN`. В ней настраиваются:

- почта для кодов подтверждения;
- кто может регистрироваться;
- токен бота Telegram для наборов стикеров;
- сеть серверов.

## 4. Каждый день

```bash
docker logs -f aquamarine-server          # журнал (Ctrl+C — выйти)
docker stop aquamarine-server             # остановить
docker start aquamarine-server            # запустить снова
```

**Обновить до новой версии** — данные в томе останутся:

```bash
docker pull ghcr.io/applememestm-pixel/aquamarine-server:latest
docker rm -f aquamarine-server
# и снова команда из раздела 2
```

С compose: `docker compose -f compose.relay.yml pull && docker compose -f compose.relay.yml up -d`.

**Резервная копия данных** в файл `aquamarine-data.tgz` в текущей папке:

```bash
docker run --rm -v aquamarine-data:/data -v "$PWD":/backup busybox tar czf /backup/aquamarine-data.tgz -C /data .
```

Метки образа:

| Метка | Что в ней |
|---|---|
| `latest` | последний полноценный выпуск |
| `1.1.0` и т. д. | конкретная версия |
| `pre` | последний пре-выпуск |
| `nightly` | свежая сборка из `main` |

## 5. Собрать образ самому

Если вы меняли код или не хотите готовый образ, Docker соберёт сервер из исходников сам. Ставить Rust и клонировать репозиторий не нужно:

```bash
docker build -t aquamarine-server -f docker/relay.Dockerfile https://github.com/applememestm-pixel/Aquamarine.git#main
```

Затем в команде из раздела 2 замените `ghcr.io/applememestm-pixel/aquamarine-server:latest` на `aquamarine-server`. Первая сборка занимает 5–15 минут.

Из своей копии репозитория: `docker build -t aquamarine-server -f docker/relay.Dockerfile .`

## 6. Если не работает

| Сообщение | Что делать |
|---|---|
| `permission denied … docker.sock` | Вы не в группе `docker`: `sudo usermod -aG docker "$USER"`, затем выйти и войти снова |
| `port is already allocated` / «Address already in use» | Порт 7777 уже занят: обычным `aqua-server`, `tools/aquamarine-server.sh` или встроенным сервером приложения. Остановите его или запустите контейнер с `-p 7778:7777` |
| `Cannot connect to the Docker daemon` | Docker не запущен: откройте Docker Desktop или `sudo systemctl start docker` |
| `no matching manifest for linux/arm/v7` | 32-битная система на ARM. Поставьте 64-битную |
| Приложение не видит сервер с телефона | Проверьте, что телефон в той же сети, адрес — IP компьютера, а не 127.0.0.1. Файрвол Windows при первом запуске спросит разрешение — разрешите для частных сетей |

Сайт проекта с документацией тоже есть в Docker — отдельный маленький образ `aquamarine-site`, см. [deployment.md, раздел 2](deployment.md#2-сайт-проекта-termux-docker).
