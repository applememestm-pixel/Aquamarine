# Aquamarine

**Мессенджер со сквозным шифрованием: настольный и мобильный клиент, самостоятельно размещаемый ретранслятор и федерация ретрансляторов.**

[Релизы](https://github.com/applememestm-pixel/Aquamarine/releases) · [Сайт](https://applememestm-pixel.github.io/Aquamarine/) · [Документация](docs/README.md) · [HTTP API](docs/api.md) · [Изменения](CHANGELOG.md) · [Безопасность](SECURITY.md)

---

## Технический паспорт

| Параметр | Значение |
|---|---|
| Назначение | Личные и групповые чаты, каналы, файлы, голосовые и видеосообщения, звонки и голосовые комнаты, боты и мини-приложения |
| Версия | 1.0.4 (клиент и ретранслятор) |
| Версия протокола | `v1` (метки KDF `Aquamarine/… v1`, API `/v1/*`) |
| Клиентские платформы | Windows 10/11, macOS (ARM и Intel), Linux x86-64, Android 7+, iOS (без подписи) |
| Платформы ретранслятора | Windows x64, macOS ARM64, Linux x86-64, Linux ARM64 |
| Языки и платформы | Rust 2021 (stable); TypeScript и Svelte 5; Tauri 2; axum, Tokio, SQLite |
| Объём исходного кода | ≈ 65 тыс. строк: Rust ≈ 24,3 тыс., TypeScript и Svelte ≈ 38,6 тыс., Python ≈ 2,1 тыс. |
| Сквозное шифрование | X3DH + Double Ratchet (X25519, Ed25519, HKDF-SHA256, ChaCha20-Poly1305); у каждого устройства свои ключи |
| Файлы | ChaCha20-Poly1305 с отдельным ключом на файл; адресация по SHA-256 шифротекста |
| Резервные копии | Формат `AQBK1`: ключ данных под Argon2id(пароль) и под ключ восстановления из 24 слов |
| Пароль | На сервер не передаётся: только производный секрет Argon2id; сервер хранит его PBKDF2 |
| Данные на ретрансляторе | Каталог и публичные профили, открытые ключи, очередь конвертов (30 сут.), шифротексты вложений (72 ч), зашифрованные копии; см. [модель угроз](docs/security-model.md) |
| Федерация | Репликация учётных записей, медиа профилей и копий за ≈ 2 с; доставка через узел получателя; переключение клиентов между узлами |
| Звонки | WebRTC напрямую между устройствами (DTLS-SRTP); групповые — mesh; объёмный звук в голосовых комнатах |
| Боты | HTTP API и совместимость с Telegram Bot API (клавиатуры, вебхуки, мини-приложения); не E2EE |
| Тесты | 124 модульных и интеграционных теста Rust, 20 тестов Python SDK, проверка типов интерфейса, проверка соответствия IPC |
| Независимый аудит | Не проводился |
| Сообщение об уязвимостях | Закрыто, через GitHub Security Advisories — [SECURITY.md](SECURITY.md) |
| Распространение | GitHub Releases; в Google Play и App Store отсутствует |
| Лицензия | MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)) |

## Устройство

```
 устройство A                                                            устройство B
 Svelte 5 ─ IPC ─ Rust (aqua-client-ui, aqua-client-core, aqua-core)     то же
 vault.sqlite (ChaCha20-Poly1305)                                        vault.sqlite
        │ HTTPS + WebSocket: только зашифрованные конверты                     │
        ▼                                                                      ▼
  aqua-server R1  ◄──── подписанная HMAC федерация ────►  aqua-server R2
  каталог, ключи, очередь, вложения, копии
 Медиапоток звонков — напрямую между устройствами (WebRTC); сигнализация — внутри E2EE.
```

| Компонент | Каталог | Назначение |
|---|---|---|
| Протокольное ядро | [`crates/aqua-core`](crates/aqua-core) | Ключи, X3DH, Double Ratchet, конверты, шифрование файлов |
| Клиентское ядро | [`crates/aqua-client-core`](crates/aqua-client-core) | Транспорт, SOCKS5, зашифрованное хранилище, резервные копии |
| Клиентское приложение | [`crates/aqua-client-ui`](crates/aqua-client-ui) | Tauri 2: команды Rust и интерфейс Svelte 5 |
| Ретранслятор | [`crates/aqua-server`](crates/aqua-server) | HTTP и WebSocket, боты, наборы стикеров, федерация, панель администратора |
| Библиотека ботов | [`sdk/python`](sdk/python) | `aquabot`: боты на Python, подключение нейросетей |

Подробно — [docs/architecture.md](docs/architecture.md).

## Документация

| Тема | Документ |
|---|---|
| Протокол и криптография | [protocol.md](docs/protocol.md) |
| Модель угроз, данные на ретрансляторе, ограничения | [security-model.md](docs/security-model.md) |
| Доставка, пересылка, автоудаление | [delivery.md](docs/delivery.md) |
| Учётные записи, устройства, пароли, восстановление | [accounts.md](docs/accounts.md) |
| Резервные копии | [backups.md](docs/backups.md) |
| Хранение на устройстве | [storage.md](docs/storage.md) |
| Файлы и медиа | [files.md](docs/files.md) |
| Группы, каналы, роли, статистика | [groups.md](docs/groups.md) |
| Звонки и голосовые комнаты | [calls.md](docs/calls.md) |
| Календарь, задачи, документы, доски | [workspace.md](docs/workspace.md) |
| Запечатанные послания | [sealed-messages.md](docs/sealed-messages.md) |
| Bluetooth | [bluetooth.md](docs/bluetooth.md) |
| Стикеры и реакции | [stickers.md](docs/stickers.md) |
| Профили, активность, игры | [profiles.md](docs/profiles.md) |
| Боты и мини-приложения | [bots.md](docs/bots.md) |
| **HTTP API ретранслятора** | [api.md](docs/api.md) |
| Федерация | [federation.md](docs/federation.md) |
| Развёртывание ретранслятора | [deployment.md](docs/deployment.md) |
| Сборка, тесты, выпуск | [building.md](docs/building.md) |
| Лицензии зависимостей | [licenses.md](docs/licenses.md) |

## Установка

Сборки публикуются в [GitHub Releases](https://github.com/applememestm-pixel/Aquamarine/releases) для каждой версии; сборки последнего коммита — в пре-релизе [nightly](https://github.com/applememestm-pixel/Aquamarine/releases/tag/nightly). К каждому релизу приложен `SHA256SUMS.txt`.

| Платформа | Клиент | Ретранслятор |
|---|---|---|
| Windows 10/11 x64 | `Aquamarine-Setup.exe` | `aqua-server-windows-x64.exe` |
| macOS Apple Silicon | `Aquamarine.dmg` | `aqua-server-macos-arm64` |
| macOS Intel | `Aquamarine-Intel.dmg` | — |
| Linux x86-64 | `Aquamarine.AppImage`, `.deb`, `.rpm` | `aqua-server-linux-x64` |
| Linux ARM64 | — | `aqua-server-linux-arm64` |
| Android | `Aquamarine.apk` | — |
| iOS | `Aquamarine-iOS-unsigned.ipa` | — |

Linux: для воспроизведения нужны `gst-plugins-good`, `gst-libav` и `ffmpeg`; для Bluetooth — `bluez-obex`.

## Свой ретранслятор

```bash
AQUA_LISTEN=0.0.0.0:7777 ./aqua-server-linux-x64
```

Сайт с документацией можно держать под рукой где угодно, хоть в Termux на телефоне: `tools/aquamarine-site.sh` (или контейнер `ghcr.io/applememestm-pixel/aquamarine-site`). Для `https://` и `wss://` перед ним ставится обратный прокси (Caddy, nginx). Переменные окружения, файлы, systemd и панель администратора — [docs/deployment.md](docs/deployment.md); объединение ретрансляторов в сеть — [docs/federation.md](docs/federation.md).

## Сборка из исходников

```bash
./build-all.sh                         # ретранслятор и клиент → ../Aquamarine-apps (Windows: build-all.bat)
cargo test                             # ядро и ретранслятор
```

Требования, Android и iOS, тесты и выпуск версии — [docs/building.md](docs/building.md). Как предложить изменение — [CONTRIBUTING.md](CONTRIBUTING.md).

## Лицензия

MIT OR Apache-2.0 по выбору получателя. Лицензии зависимостей и встроенных ресурсов — [docs/licenses.md](docs/licenses.md).
