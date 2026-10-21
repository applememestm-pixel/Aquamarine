# Сборка, тестирование и выпуск

> Часть [технической документации](README.md). Правила участия — [CONTRIBUTING.md](../CONTRIBUTING.md).

## 1. Требования

| Компонент | Версия |
|---|---|
| Rust | stable (см. `rust-toolchain.toml`) |
| Node.js | 22 или новее |
| Tauri CLI | 2 (`cargo install tauri-cli --version '^2' --locked`) |
| Linux | WebKitGTK 4.1, GTK 3, librsvg, patchelf; для воспроизведения — GStreamer ([files.md](files.md#5-зависимости-linux)) |
| Windows | MSVC Build Tools («Desktop development with C++»), WebView2 (есть в Windows 10/11); для MSI — WiX |
| macOS | Xcode Command Line Tools |

```bash
sudo pacman -S --needed rustup nodejs npm base-devel webkit2gtk-4.1 libappindicator-gtk3 librsvg patchelf   # Arch Linux
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf                                   # Debian, Ubuntu
```

## 2. Сборка

```bash
./build-all.sh                                    # всё → ../Aquamarine-apps (Windows: build-all.bat)
cargo build --release -p aqua-server              # только ретранслятор → target/release/aqua-server
cd crates/aqua-client-ui/ui && npm ci && npm run build && cd .. && cargo tauri build   # клиент с установщиками
```

- `build-all` собирает ретранслятор и клиент и копирует их вместе со сценариями запуска (`tools/aquamarine.sh`, `tools/aquamarine-server.sh`, `tools/server-panel.sh`) в соседний каталог `Aquamarine-apps`.
- Клиентский крейт `crates/aqua-client-ui` не входит в рабочее пространство Cargo: `tauri-build` требует собранный интерфейс на этапе компиляции. Он собирается из своего каталога и имеет свой `target/`.
- Сборка без установщиков: `cargo build --release --features custom-protocol` в `crates/aqua-client-ui`.
- Релизный профиль — thin LTO, 4 единицы кодогенерации.

### 2.1. Android

Сценарии в `android/` ([android/README.md](../android/README.md)):

```bash
bash android/arch-all.sh          # Arch Linux: зависимости, SDK и NDK, APK
android/1-setup.sh                # один раз: цели Rust, проект Gradle, разрешения
android/3-build-apk.sh            # → android/out/Aquamarine-debug.apk
```

### 2.2. iOS

`cargo tauri ios build` на macOS. Без учётной записи разработчика Apple сборка не подписывается; `.ipa` устанавливается через AltStore или SideStore.

### 2.3. Linux: запуск

`tools/aquamarine.sh` запускает клиент, ведёт журналы и задаёт обходы для драйверов (на NVIDIA в Wayland — `WEBKIT_DISABLE_DMABUF_RENDERER=1`). Формат AppImage в Arch требует пакет `fuse2`.

## 3. Тестирование

```bash
cargo test                                         # aqua-core, aqua-client-core, aqua-server (e2e, федерация, наборы)
cd crates/aqua-client-ui && cargo test --lib       # клиент: история, группы и права, медиа, автоудаление
python3 tools/check-invokes.py                     # каждый вызов invoke() есть среди команд Rust
cd crates/aqua-client-ui/ui && npx svelte-check    # типы интерфейса
cd sdk/python && python -m pytest                  # библиотека ботов
```

| Набор | Что проверяет |
|---|---|
| `crates/aqua-core` | X3DH, Double Ratchet (включая внеочередные сообщения), форматы, шифрование файлов |
| `crates/aqua-client-core` | Хранилище, резервные копии, ключ восстановления, транспорт |
| `crates/aqua-server/tests/e2e.rs` | Полный цикл двух клиентов через ретранслятор; отказ без токена и с неверной подписью; медиа профиля; боты и слой Telegram; мини-приложения; резервная копия и перенос; два устройства; конфликт адреса и сброс пароля; запечатанные послания |
| `crates/aqua-server/tests/federation.rs` | Два узла: репликация, доставка, переключение |
| `crates/aqua-server/tests/packs_*.rs` | Импорт и обновление наборов (с поддельным Telegram) |
| `crates/aqua-client-ui` | История и сжатие, снимки и права групп, роли, временные комнаты, пересылка, служебные префиксы, клавиатуры ботов, ссылки и статьи, определение активности, наборы стикеров, локальный медиасервер |

## 4. Непрерывная интеграция

| Рабочий процесс | Когда | Что делает |
|---|---|---|
| `release.yml` | Тег `v*`; каждый push в `main`; вручную | Тег — стабильный релиз; push — пре-релиз `nightly`. Собирает клиент (Windows, macOS ARM и Intel, Linux), Android (APK), iOS (неподписанный `.ipa`), ретранслятор (Windows, macOS, Linux x86-64 и ARM64); прикладывает `SHA256SUMS.txt`; публикует образ ретранслятора `ghcr.io/…/aquamarine-server` (amd64, arm64) |
| `autotag.yml` | Push в `main` | Если версии из `Cargo.toml` нет тега `v<версия>`, создаёт тег и запускает `release.yml` на нём |
| `windows-build.yml` | Вручную | Отдельная сборка `aqua-server.exe` и клиента под Windows |
| `pages.yml` | Изменения `site/`, `docs/`, `README.md` | Публикация сайта на GitHub Pages |
| `sdk-python.yml` | Изменения `sdk/python/`; тег `sdk-py-v*` | Тесты библиотеки ботов, сборка пакета |

Подпись APK берётся из секретов `ANDROID_KEYSTORE_B64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`; без них используется одноразовый ключ (обновление потребует переустановки).

## 5. Выпуск версии

1. Поднять версию в `Cargo.toml` (`[workspace.package]`), `crates/aqua-client-ui/Cargo.toml`, `tauri.conf.json`, `ui/package.json`.
2. Описать изменения в `docs/releases/v<версия>.md` и добавить строку в [CHANGELOG.md](../CHANGELOG.md).
3. Отправить в `main`. `autotag.yml` создаст тег, `release.yml` соберёт релиз и возьмёт описание из `docs/releases/`.
