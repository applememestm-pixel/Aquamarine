# MODULE_INVENTORY

Карточки ключевых модулей. Формат: назначение · технологии · вход · зависимости · данные · связи · состояние · ограничения · проблемы.

Полная построчная инвентаризация (больше 1000 исходных файлов Aquamarine) не делалась. Ниже — модули, через которые проходят безопасность, доставка, хранение и производительность. Остальное заносится по мере аудита соответствующих областей.

## Aquamarine — `crates/aqua-core` (протокол)

| Модуль | Назначение | Состояние | Проблемы |
|---|---|---|---|
| `crypto.rs` | X25519, Ed25519, HKDF, HMAC, ChaCha20-Poly1305, zeroize | рабочее | — |
| `keys.rs` | IdentityKeyPair, SignedPreKey, OneTimePreKey, PreKeyBundle | рабочее | — |
| `x3dh.rs`, `ratchet.rs`, `session.rs` | Собственная реализация X3DH и Double Ratchet: `MAX_SKIP` = 1000, откат состояния при ошибке (`rollback_point`) | рабочее, своя криптография | Кэш пропущенных ключей чистится через `Vec::remove(0)` за O(n) (P2); независимого криптоаудита не было |
| `wire.rs` | Формат обмена; `auth::dh_proof` (1.21.0-23) | рабочее | — |
| `devlist.rs` | Подписанный список устройств | рабочее | `verify_self` проверяет ключом из самого JSON; на relay закрыто через `identity_signs` (SEC-001) |
| `file_transfer.rs` | Шифрование файлов частями, ключ на файл | рабочее | — |

## Aquamarine — `crates/aqua-server` (relay)

| Модуль | Назначение | Вход | Проблемы |
|---|---|---|---|
| `lib.rs` | AppState, роутер, WS relay, bundle, blobs, signup/signin, admin, bot API | `app()`, `AppState::new*` | SEC-001/002/004/006 исправлены; SEC-007, SEC-011 открыты |
| `auth.rs` | challenge/verify, HMAC-токен | `/v1/auth/*` | SEC-001 исправлен |
| `directory.rs` | SQLite: аккаунты, коды, inbox, prekeys, история, бэкапы, баны, устройства | `Directory::open` | Один `Mutex<Connection>`, около 121 `lock().unwrap()` в синхронном коде внутри async-обработчиков (P2); SEC-003/005 исправлены |
| `fed.rs`, `fedtunnel.rs` | Федерация и обратный туннель | `/v1/fed/*` | SEC-009, SEC-010, SEC-011 |
| `botx.rs`, `tgapi.rs`, `botmaker.rs` | Нативный Bot API, совместимость с Telegram Bot API, конструктор ботов | `/bot*` | Закрепления в `tgapi` — заглушка (`pin*` возвращает `true`) |
| `blobstore.rs`, `blob_upload.rs` | Зашифрованные тела файлов, загрузка частями | `/v1/blob*` | — |
| `totp.rs`, `tglogin.rs` | 2FA, вход через Telegram | — | — |
| `turnsrv.rs` | Встроенный TURN для звонков | — | не проверялся |
| `selfupdate.rs` | Самообновление консольного сервера | — | SEC-012 |

## Aquamarine — `crates/aqua-client-core`

| Модуль | Назначение | Проблемы |
|---|---|---|
| `transport.rs` | HTTP/WS, вход (`sign_in` + `sign_in_proof`), SOCKS5 | Предупреждение `unused mut` на стр. 120 (было до изменений) |
| `vault.rs` | Записи, запечатанные ChaCha20-Poly1305; `MasterKey` с zeroize | Метаданные SQLite не шифруются |
| `prekeys.rs` | Prekeys клиента | — |

## Aquamarine — `crates/aqua-client-ui`

| Часть | Назначение | Проблемы |
|---|---|---|
| `src/commands.rs` | IPC-команды: отправка, приём, расшифровка, `get_messages` | `get_messages` отправляет всю историю по IPC при каждом обновлении (P2); OPK удаляется до `accept_initial` (SEC-008) |
| `src/multidev.rs`, `devtrust.rs`, `devlists.rs` | Несколько устройств, доверие, списки | Набор replay только в памяти (SEC-008) |
| `src/state.rs` | Состояние, мастер-ключ (keyring или файл) | Android/iOS: открытый `master.key` (SEC-013) |
| `src/bgnotify.rs` | Уведомления через tauri-plugin-notification | Своего канала для сообщений нет (используется канал плагина по умолчанию) |
| `ui/src/lib/components/ChatWindow.svelte` | Окно чата, 7 510 строк | Пересчёт производных по всей истории; закрепления на чат, а не на тему |
| `ui/src/lib/messageMeta.ts` | Закрепления (localStorage) и хеш постов для комментариев | Закрепления не синхронизируются; комментарии привязаны к FNV-1a хешу текста |

## Aquamarine — Android (`android/*.kt`)

- `AquaRelay.kt` — `AquaSyncService`: foreground service типа `REMOTE_MESSAGING` (API 34+), `START_STICKY`, каналы `sync` и `relay`. FCM не используется.
- Нет `BOOT_COMPLETED`: после перезагрузки служба не поднимается, пока не откроют приложение.
- Служба и FGS-разрешения добавляются в манифест только в `release.yml`. Локальные `patch-project.*` их не добавляют.

## CeZeOs

| Файл | Назначение | Проблемы |
|---|---|---|
| `server.mjs` (578 строк) | HTTP, роутер, API, статика, заголовки безопасности | Лента без LIMIT с фильтрацией в JS, N+1 (7–11 запросов на пост), нет пагинации, поиск профилей загружает всё, нет rate limit; CZ-SEC-001 исправлен |
| `store.mjs` | Схема SQLite, pragmas WAL, seed | Нет миграций (`user_version`); нет индексов `posts(author/group_id/kind)` и `reactions(post)` |
| `region.mjs` | Ключ региона Ed25519, подписанный дескриптор и лента, проверка пиров | Сделано добротно: закрепление по отпечатку, ±120 с, лимит 512 KB |
| `telegram.mjs` | Telegram OIDC: code + PKCE S256, state, nonce, JWKS RS256 | Попытки хранятся в памяти процесса |
| `ecosystem.mjs` | Привязка к Aquamarine | Подписанта на стороне Aquamarine нет |
| `governance.mjs` | Triumvirate и Synclit | — |
| `operator.mjs`, `update.mjs` | Бэкап, назначение триумвиров, обновление Docker с откатом | — |
| `public/*` | SPA, PWA | — |
| `*.test.mjs` | `node --test`, 10 тестов | Нет тестов прав wiki, лимитов загрузки, rate limit |
