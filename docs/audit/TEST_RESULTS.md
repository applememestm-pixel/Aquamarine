# TEST_RESULTS

Только то, что реально запускалось. Дата: 2026-11-25. Машина: облачный контейнер Linux, Rust из `rust-toolchain.toml`, Node 22.23.4.

## Aquamarine — локально (офлайн, без `aqua-stt`)

Команда:

```
cargo test --offline -p aqua-server -p aqua-core -p aqua-client-core
```

Для офлайн-сборки член workspace `crates/aqua-stt` временно закомментирован, а `pin-utils` в Cargo.lock приведён к версии из кэша (0.1.1). В коммиты эти локальные правки не попадают.

| Набор | Базовый прогон (до изменений, `1268b604`) | После P0 + слияния `main` (`94e0401a`) |
|---|---|---|
| aqua-core lib | 43 ok | 44 ok |
| aqua-client-core lib | 24 ok | 24 ok |
| aqua-server lib | 62 ok | 70 ok (+ `dh_proof`, `pending_is_capped`, OTP; тест минимальной версии обновлён) |
| aqua-server e2e | 17 ok | 19 ok (+ подделка токена, bundle с недоказанным ключом) |
| aqua-server federation | 5 ok | 5 ok (`solo_relay_shares_only_names` дополнен проверкой AQUA-SEC-002) |
| прочие (embedded, packs_*) | ok | ok |

Падений нет. Три падения во время работы — это ожидаемые изменения поведения, исправлены в тестах:
- лимит повторной отправки в dev-режиме;
- гашение ticket при сбросе пароля откачено, потому что ломало сценарий «сбросить и войти»;
- тест «пароль после сброса».

## CeZeOs — локально

Команда: `npm test` (`node --test`).

10/10 ok, включая 3 новых в `production.test.mjs`. Тесты 1 и 3 из `production.test.mjs` запускались и на старом коде (`git stash`) — там они падают.

## Не запускалось (с причиной)

| Что | Причина |
|---|---|
| `aqua-client-ui` (`cargo test --lib`, svelte-check, сборка UI) | npm и `ort` недоступны офлайн; проверяется в CI PR #81 |
| `aqua-server-app` | Отдельный workspace, в кэше нет зависимостей Tauri; проверяется в CI |
| Android (Kotlin, устройство) | Нет устройства и SDK |
| Нагрузочные тесты, fuzzing | Ещё не написаны (этап P1/P2) |
| UI-снимки, FPS | Только в CI (`ui screenshots`), замеров на железе нет |
