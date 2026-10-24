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
| `aquamarine-site.sh` | Сайт проекта с документацией на `:8080` — на компьютере, в Termux на телефоне или в Docker (раздел 2) |

## 2. Сайт проекта: Termux, Docker

Ретранслятор в контейнер не упаковывается — он запускается как обычная программа (раздел 1). Отдельно можно держать под рукой сайт проекта с документацией и справочником API; он же всегда доступен на [GitHub Pages](https://applememestm-pixel.github.io/Aquamarine/).

`tools/aquamarine-site.sh` отдаёт сайт на `http://<адрес>:8080/` (`PORT=…` — другой порт). Из копии репозитория берёт файлы рядом, иначе скачивает `aquamarine-site.tar.gz` из релиза (`TAG=latest` — стабильный, по умолчанию `nightly`). Нужен `python3` (или `busybox`, или `php`).

Termux на телефоне:

```bash
pkg install curl python
curl -fsSLO https://raw.githubusercontent.com/applememestm-pixel/Aquamarine/main/tools/aquamarine-site.sh
bash aquamarine-site.sh            # update — скачать свежую версию
```

Docker (amd64, arm64, armv7; образ ~2 МБ на busybox):

```bash
bash aquamarine-site.sh docker     # фоном, поднимается после перезагрузки; stop — остановить
# или вручную
docker run -d --name aquamarine-site --restart unless-stopped -p 8080:8080 ghcr.io/applememestm-pixel/aquamarine-site
# или из исходников
docker compose up -d --build
```

Кнопки «Скачать» на сайте ведут на последний релиз GitHub.

## 3. TLS

Ретранслятор говорит по HTTP. Для `https://` и `wss://` перед ним ставится обратный прокси. Пример для Caddy (сертификат выпускается автоматически):

```
relay.example.org {
    reverse_proxy 127.0.0.1:7777
}
```

Для nginx нужно пробросить заголовки `Upgrade` и `Connection` для `/v1/ws` и поднять `client_max_body_size` до 220m (вложения до 210 МиБ).

## 3a. Доступ из интернета по белому IP

Ретранслятор дома, на компьютере за роутером:

1. Запуск через `aquamarine-server.sh` (слушает `0.0.0.0:7777`).
2. На роутере — проброс TCP 7777 на этот компьютер (в Keenetic: «Переадресация портов»; компьютеру лучше закрепить постоянный IP в списке клиентов).
3. Открыть порт во внутреннем брандмауэре, если он есть: `sudo ufw allow 7777/tcp` или `sudo firewall-cmd --add-port=7777/tcp --permanent && sudo firewall-cmd --reload`.

Ретранслятор сам узнаёт свой адрес в интернете и печатает его строкой `Internet  http://<IP>:7777`. Этот адрес отдаётся клиентам в `GET /v1/fed/peers` (поле `self`): устройство, вошедшее дома по `http://192.168.x.x:7777`, запоминает его и, оказавшись вне домашней сети, переключается на него само. Новые устройства указывают `http://<IP>:7777` в поле «Сервер» при входе.

Белый IP есть, только если адрес WAN в роутере совпадает с тем, что показывает, например, 2ip.ru. Адреса `10.x`, `100.64–127.x`, `172.16–31.x`, `192.168.x` на WAN — серый IP (NAT провайдера), проброс снаружи не сработает: нужна услуга «белый IP» у провайдера или VPS. Если IP динамический, удобно имя KeenDNS (`<имя>.keenetic.pro`) или DNS-запись своего домена.

Белого IP нет, а VPS не хочется — ретранслятор можно открыть друзьям через виртуальную сеть (NetBird, ZeroTier, Tailscale, Radmin VPN, Hamachi): [overlay-networks.md](overlay-networks.md).

Трафик по `http://` не защищён TLS: сообщения и файлы зашифрованы сквозным шифрованием, но для постоянной работы лучше домен и TLS (раздел 3).

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
| `AQUA_PUBLIC_URL` | Публичный адрес данного узла; `off` — не определять | При `AQUA_LISTEN=0.0.0.0:…` — `http://<белый IP>:<порт>`, определяется сам |
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
| `AQUA_SITE_DIR` | Каталог статического сайта: всё, что не занято API, отдаётся оттуда (`/` → `index.html`) | — (сайт выключен) |

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
