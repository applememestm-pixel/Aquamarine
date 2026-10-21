# Развёртывание ретранслятора

> Часть [технической документации](README.md). Методы — [api.md](api.md); федерация — [federation.md](federation.md).

## 1. Запуск

Ретранслятор — один исполняемый файл без внешних зависимостей (SQLite встроена). Сборки для Windows x64, macOS ARM64, Linux x86-64 и ARM64 — в [GitHub Releases](https://github.com/applememestm-pixel/Aquamarine/releases).

```bash
AQUA_LISTEN=0.0.0.0:7777 ./aqua-server-linux-x64
```

Ретранслятор создаёт файлы данных в текущем каталоге (раздел 4). При первом запуске без токена Telegram он спрашивает его в консоли (`AQUA_TG_PROMPT=0` отключает вопрос).

Сценарии из `tools/`:

| Сценарий | Назначение |
|---|---|
| `aquamarine-server.sh`, `Aquamarine Server.bat` | Запуск свежей сборки ретранслятора |
| `server-panel.sh` | Ретранслятор с текстовой панелью: адрес для подключения, счётчики `/v1/stats`, журнал (коды подтверждения в режиме разработки) |
| `aquamarine-docker.sh` | Ретранслятор в Docker из `ghcr.io` (по умолчанию `:nightly`, `TAG=latest` — стабильный): обновляет образ, пересоздаёт контейнер, показывает адрес и журнал |

## 2. Docker

Образ публикуется в GitHub Container Registry: `ghcr.io/applememestm-pixel/aquamarine-server` — `<версия>` и `latest` с каждым релизом, `nightly` с каждым push в `main`; amd64 и arm64.

```bash
docker run -d --name aquamarine --restart unless-stopped \
  -p 7777:7777 -v aquamarine-data:/data \
  ghcr.io/applememestm-pixel/aquamarine-server:latest
```

Из исходников — `docker-compose.yml` в корне репозитория:

```bash
docker compose up -d --build      # первая сборка ~5 мин
docker compose logs -f            # журнал; коды подтверждения почты — здесь, пока не настроен SMTP
```

- Данные (база, вложения, наборы, настройки) — в томе `/data`. Процесс работает от пользователя `aqua` (UID 10001); при монтировании каталога хоста вместо тома выдайте ему права: `chown 10001 <каталог>`.
- Переменные окружения — как в разделе 5 (`-e AQUA_ADMIN_TOKEN=…`, `-e AQUA_PUBLIC_URL=…`).
- Проверка состояния встроена (`HEALTHCHECK` по `/v1/stats`).
- Обновление: `docker pull …:latest`, затем пересоздать контейнер; том сохраняется.

## 3. TLS

Ретранслятор говорит по HTTP. Для `https://` и `wss://` перед ним ставится обратный прокси. Пример для Caddy (сертификат выпускается автоматически):

```
relay.example.org {
    reverse_proxy 127.0.0.1:7777
}
```

Для nginx нужно пробросить заголовки `Upgrade` и `Connection` для `/v1/ws` и поднять `client_max_body_size` до 220m (вложения до 210 МиБ).

## 4. Служба systemd

```ini
[Unit]
Description=Aquamarine relay
After=network-online.target

[Service]
WorkingDirectory=/var/lib/aquamarine
Environment=AQUA_LISTEN=127.0.0.1:7777
Environment=AQUA_PUBLIC_URL=https://relay.example.org
Environment=AQUA_TG_PROMPT=0
EnvironmentFile=-/etc/aquamarine/relay.env
ExecStart=/usr/local/bin/aqua-server
Restart=on-failure
DynamicUser=yes
StateDirectory=aquamarine

[Install]
WantedBy=multi-user.target
```

Секреты (`AQUA_ADMIN_TOKEN`, `AQUA_FED_SECRET`, `AQUA_SMTP_PASS`, `AQUA_TG_BOT_TOKEN`) — в `relay.env` с правами `0600`.

## 5. Переменные окружения

| Переменная | Назначение | По умолчанию |
|---|---|---|
| `AQUA_LISTEN` | Адрес и порт | `127.0.0.1:7777` |
| `AQUA_DIR_DB` | Файл базы данных SQLite | `aqua-directory.sqlite` |
| `AQUA_BLOB_DIR` | Каталог шифротекстов | `aqua-blobs` |
| `AQUA_BLOB_TTL_HOURS` | Срок хранения вложений, ч | `72` |
| `AQUA_FED_SECRET` | Общий секрет федерации | — |
| `AQUA_FED_PEERS` | Адреса узлов федерации через запятую | — |
| `AQUA_PUBLIC_URL` | Публичный адрес данного узла | — |
| `AQUA_ADMIN_TOKEN` | Токен панели администратора | Случайный при каждом запуске |
| `AQUA_ADMIN=off` | Отключение панели | — |
| `AQUA_OPEN_PANEL=1` | Открыть панель в браузере при запуске | — |
| `AQUA_OPEN_SIGNUP=1` | Не требовать регистрации имени | — |
| `AQUA_SMTP_HOST`, `_PORT`, `_USER`, `_PASS`, `_FROM` | Параметры SMTP | Из `aqua-smtp.json` |
| `AQUA_DEV_CODES` | `1` — всегда выводить коды в консоль; `0` — никогда | Автоматически |
| `AQUA_TG_BOT_TOKEN` | Токен Telegram Bot API для импорта наборов | Из `aqua-telegram.json` |
| `AQUA_TG_PROMPT=0` | Не спрашивать токен Telegram при запуске | — |
| `AQUA_DEFAULT_PACKS` | Наборы, импортируемые при запуске | `eltaller47` |
| `AQUA_GAMES_CACHE` | Файл каталога игр | `aqua-games.json` |
| `AQUA_HOST_PORT`, `AQUA_LAN_URL` | Порт и адрес в локальной сети для заставки при запуске в контейнере (задаёт `aquamarine-docker.sh`) | Порт из `AQUA_LISTEN`, адрес определяется сам |

## 6. Файлы

| Файл | Содержимое |
|---|---|
| `aqua-directory.sqlite` | Каталог, очередь `inbox`, резервные копии, открытые ключи (`prekeys`), ключи ботов, медиа профилей, запечатанные послания, время последнего подключения |
| `aqua-blobs/` | Шифротексты вложений; `keep/` — долгоживущие |
| `aqua-packs/` | Наборы стикеров |
| `aqua-games.json`, `aqua-games/` | Каталог игр, значки и обложки |
| `aqua-admin.json`, `aqua-smtp.json`, `aqua-telegram.json`, `aqua-federation.json`, `aqua-tg-webhooks.json` | Настройки |

Резервное копирование ретранслятора — копия `aqua-directory.sqlite` (лучше `sqlite3 aqua-directory.sqlite ".backup …"`) и каталогов `aqua-blobs/keep`, `aqua-packs/`.

## 7. Панель администратора

`/admin` при заданном `AQUA_ADMIN_TOKEN`: статистика, режим «только по приглашению» с белым списком адресов, SMTP с пробным письмом, токен Telegram, федерация. Без настроенного SMTP ретранслятор работает в режиме разработки: коды подтверждения видны в консоли и панели.

## 8. Обновление

Остановить службу, заменить исполняемый файл, запустить. Схема базы данных обновляется при запуске; клиенты переподключаются сами. Новые функции клиента (мини-приложения, запечатанные послания, временные комнаты, обложки игр) требуют ретранслятор той же или более новой версии.
