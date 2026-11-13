# CLAUDE.md

Только правила и ловушки. Что и где менялось — `CHANGELOG.md` и код; сюда не пересказывать. Дописывай строку, только если без неё что-то сломается.

## Работа с владельцем
- Пишет по-русски, коротко, со скриншотами. Отвечай по-русски, по делу.
- Работа копится в одном открытом PR (одна версия «Pre-release X.Y.Z-N», шаблон `.github/pull_request_template.md`). Не проси мёржить после каждой правки: мёрж — только когда владелец скажет (тогда можно и самому, `gh pr merge`). Без force-push. Смёржен — новая ветка от свежего `main`.
- В PR: версия, `CHANGELOG.md`, `docs/releases/vX.md` (это текст окна «Что нового» — для людей).
- Строки UI — по-английски через `$t()`/`tr()`, перевод в `crates/aqua-client-ui/ui/src/lib/i18n.ru.ts` (без дублей ключей).
- Секреты в репозиторий не попадают; секрет в чате → посоветовать сменить.
- `NavRail` (основная нижняя панель) не трогать (исключение владельца: вкладка «Музыка», только с дополнением Navidrome). `MIN_VERSION` (`aqua-server/src/lib.rs`) поднимать только по прямой просьбе.

## Проверка (npm и crates.io локально закрыты — всё в CI)
- `.github/workflows/checks.yml`. Лог: `gh api repos/applememestm-pixel/Aquamarine/actions/jobs/<id>/logs`.
- Снимки UI: задача `ui screenshots (log)` → `tools/ui-shots.js` (base64 JPEG между `=====SHOT`/`=====END`).
- Панель сервера (`crates/aqua-server-app/ui/index.html`) — Playwright с моком `window.__TAURI__`, **с CSP с nonce**, как у Tauri.
- «Упал выпуск»: пустые шаги или HTTP 5xx при выкладке — это GitHub, `release-retry.yml` перезапустит.

## Версии
- Менять: `Cargo.toml`, `crates/aqua-client-ui/{Cargo.toml,Cargo.lock,tauri.conf.json,ui/package.json,ui/package-lock.json}`, `crates/aqua-server-app/{Cargo.toml,Cargo.lock,tauri.conf.json}`, `Cargo.lock`, `README.md`, `docs/README.md`, `docs/api.md`, `site/index.html`, `.github/ISSUE_TEMPLATE/bug_report.yml` (`sed` старой версии).
- Схема: `X.Y.0-N` → выпуск `X.Y.1` → `X.Y.11`, `X.Y.12`… Сравнение: `ui/src/lib/updates.ts::newer` (= `aqua-server-app/src/updates.rs::newer`). Пре-выпуск: `[workspace.metadata.aqua] prerelease = true`.
- RPM не принимает «-»: для него `X.Y.0+N` (`release.yml`).
- Android `versionCode` — только `tools/android_version_code.py` (порядок как у `newer`); правило Tauri делало 1.2.2 «ниже» 1.2.11, и обновление не ставилось.

## Где что
- `crates/aqua-core` — протокол; `crates/aqua-client-core` — транспорт/хранилище.
- `crates/aqua-client-ui` — мессенджер (отдельный workspace): Rust `src/`, Svelte `ui/src/lib`.
- `crates/aqua-server` — ретранслятор; `crates/aqua-server-app` — его приложение (отдельный workspace).
- `crates/aqua-nsfw` — детектор 18+ (сервер и клиент). `model/aqnsfw.bin` — только из `tools/nsfw_convert.py` (тест сверяет с эталоном; после пересборки обновить `REFERENCE` через `--pattern`).

## Ловушки
- Новый формат сообщения → `compat.ts::KNOWN`. Служебная приставка → `commands.rs::is_control_text`.
- Удаление сообщений → `history_sync::tombstone`. Отправка своим устройствам → `multidev::send_to_own`.
- Нерасшифрованное сообщение не отбрасывать молча — `multidev::repair_session` (копии узнаёт `already_decrypted`). `Closed` старого `Transport` игнорируется (`connect_to_server`), `Drop` гасит его задачи — иначе связь «мигает».
- Списки — только `Select.svelte`. Скругления — `calc(Npx * var(--rk, 1))`. Окна: крестик `data-x`, панель `data-back` (`lib/back.ts`).
- «Сложная» настройка → `SettingsPanel.svelte::ADVANCED`. Платформа — `lib/platform.ts` (`IS_MOBILE`/`IS_DESKTOP`), не ширина окна.
- Цвета — только через переменные темы (`theme.ts`), карточка профиля — `cardStyle.ts::cardLook` + класс `.light`.
- Сервер: `fed::current()` — полная сеть, `fed::registry()` — любая; соло отдаёт только имена. Метка mesh `VIA` живёт в своей задаче — не `spawn`.
- Панель сервера: `onclick="…"` работают только через `armAll`; блоки из `poll` перерисовывать только при изменении (`dataset.h`); новая настройка в `aqua-server/src/admin.html` → и сюда.
- Сервер при остановке стирает кэш файлов (`BlobStore::clear_cache`, кроме `keep/`, только файлы верхнего уровня) — так решил владелец; данные людей и наборы стикеров/эмодзи (`packs::PACKS_DIR`, отдельная папка) не трогать.
- Navidrome: логин сервера людям не отдаётся — только `/v1/music/*` насквозь (`aqua-server/src/music.rs`); звук в клиенте — через `media_server::remote_url`, не прямыми ссылками (CSP).
- Mesh (`mesh.rs`): новый кадр — только за флагом в `hello.caps`, иначе старые версии рвут сессию на «bad frame».
- Обновления на Windows — тихий NSIS `/S` + перезапуск (`src/updates.rs` в обоих приложениях); без строки в `SHA256SUMS.txt` не ставится.
- Обновления обязательны (владелец): выпуски ставятся сами везде, «Позже»/выключатель не возвращать; пре-выпуски — только «Тестер». Ночных сборок нет; канал тестера — наибольший номер тега, не `published_at`. Консольный сервер — `selfupdate.rs` (имена файлов выпуска `aqua-server-<os>-<arch>` не менять).
- Панель Aquamarine Server ходит в сервер только через `admin(…, target)`; удалённый — SSH-туннель (`remote.rs`, порт/токен из `panel.json`, `host::write_panel_file`).

## Открыто
- Не проверено на устройствах: биометрия, сканер QR, список устройств, автообновление, Navidrome (живого сервера в CI нет), показ экрана с Android (MediaProjection), файлы по Wi-Fi/Bluetooth. Kotlin (`android/*.kt`) в CI на PR не собирается — только в `release.yml`.
- Не сделано: конструктор ботов. Android с Wi-Fi без интернета: сокеты идут по сотовой сети по умолчанию (нужен `bindProcessToNetwork`/привязка сокета) — LAN может не находить соседей. FLAG_SECURE (`screen_secure`) не проверен на устройстве.
