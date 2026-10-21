# Лицензирование и сторонние компоненты

> Часть [технической документации](README.md).

## 1. Лицензия проекта

Исходный код Aquamarine распространяется на условиях **MIT OR Apache-2.0** по выбору получателя. Тексты лицензий приведены в файлах [LICENSE-MIT](../LICENSE-MIT) и [LICENSE-APACHE](../LICENSE-APACHE).

Вклад в проект, намеренно представленный для включения, лицензируется на тех же условиях без дополнительных требований (Apache-2.0, раздел 5), если явно не оговорено иное.

## 2. Прямые зависимости Rust

| Крейт | Версия | Лицензия | Компонент |
|---|---|---|---|
| `x25519-dalek` | 2.0.1 | BSD-3-Clause | core |
| `ed25519-dalek` | 2.2.0 | BSD-3-Clause | core |
| `curve25519-dalek` (транзитивно) | 4.1.3 | BSD-3-Clause | core |
| `subtle` | 2.6.1 | BSD-3-Clause | core, server |
| `chacha20poly1305` | 0.10.1 | Apache-2.0 OR MIT | core, client-core |
| `hkdf`, `hmac`, `sha2` | 0.12.4, 0.12.1, 0.10.9 | MIT OR Apache-2.0 | все |
| `argon2` | 0.5.3 | MIT OR Apache-2.0 | client-core |
| `zeroize` | 1.9.0 | Apache-2.0 OR MIT | core, client-core |
| `rand_core`, `rand` | 0.6.4, 0.8.8 | MIT OR Apache-2.0 | все |
| `flate2` | 1.1.10 | MIT OR Apache-2.0 | client-core, client-ui |
| `serde`, `serde_json`, `serde_bytes` | 1.0.229, 1.0.151, 0.11.19 | MIT OR Apache-2.0 | все |
| `bincode` | 1.3.3 | MIT | все |
| `base64`, `hex` | 0.22.1 и 0.21.7, 0.4.3 | MIT OR Apache-2.0 | все |
| `thiserror`, `anyhow` | 1.0.69, 1.0.104 | MIT OR Apache-2.0 | все |
| `tokio` | 1.53.1 | MIT | все |
| `axum` | 0.7.9 | MIT | server |
| `tower` | 0.5.3 | MIT | server |
| `dashmap` | 6.2.1 | MIT | server |
| `rusqlite` (со встроенной SQLite) | 0.32.1 | MIT; SQLite — общественное достояние | client-core, server |
| `lettre` | 0.11.23 | MIT | server |
| `reqwest` | 0.12.28 | MIT OR Apache-2.0 | все |
| `tokio-tungstenite` | 0.24.0 | MIT | client-core |
| `tokio-socks` | 0.5.3 | MIT | client-core |
| `futures-util` | 0.3.34 | MIT OR Apache-2.0 | все |
| `tracing`, `tracing-subscriber` | 0.1.44, 0.3.23 | MIT | все |
| `nu-ansi-term` | 0.50.3 | MIT | server |
| `uuid` | 1.26.1 | Apache-2.0 OR MIT | server |
| `tauri` | 2.12.0 | Apache-2.0 OR MIT | client-ui |
| `tauri-plugin-dialog`, `-fs`, `-log` | 2.8.0, 2.6.0, 2.10.0 | Apache-2.0 OR MIT | client-ui |
| `tiny_http` | 0.12.0 | MIT OR Apache-2.0 | client-ui |
| `infer` | 0.22.0 | MIT | client-ui |
| `dirs`, `log` | 5.0.1, 0.4.34 | MIT OR Apache-2.0 | client-ui |
| `keyring` | 3.6.3 | MIT OR Apache-2.0 | client-ui (настольные ОС) |
| `webkit2gtk`, `zbus` | 2.0.2, 5.19.0 | MIT | client-ui (Linux) |
| `windows` | 0.62.2 | MIT OR Apache-2.0 | client-ui (Windows) |

## 3. Транзитивные зависимости Rust

Сводка получена из `cargo metadata`: учтены зависимости сборки и выполнения, dev-зависимости исключены.

| Граф | Пакетов | MIT и/или Apache-2.0 | Unicode-3.0 | BSD-3-Clause | MPL-2.0 | ISC | CDLA-Permissive-2.0 | Прочие разрешительные |
|---|---|---|---|---|---|---|---|---|
| Рабочее пространство (core, client-core, server) | 237 | 206 | 18 | 4 | 0 | 2 | 2 | 5 |
| Клиентское приложение (с Tauri) | 566 | 526 | 18 | 6 | 5 | 3 | 2 | 6 |

Примечания:

- **MPL-2.0** — пакеты `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short` и `option-ext` (зависимости Tauri). Они используются без изменений. Условия MPL-2.0 действуют на уровне файлов и не распространяются на код Aquamarine.
- **Прочие разрешительные** — Zlib, 0BSD и составные выражения:
  - `ring`: Apache-2.0 AND ISC;
  - `brotli`: BSD-3-Clause AND MIT;
  - `dpi`: Apache-2.0 AND MIT;
  - `unicode-ident`: (MIT OR Apache-2.0) AND Unicode-3.0.
- `r-efi` предлагает LGPL-2.1-or-later только как одну из альтернатив наряду с MIT и Apache-2.0.
- Лицензий с копилефтом сильнее MPL-2.0 в графе нет.

## 4. Зависимости интерфейса (npm)

| Пакет | Версия | Лицензия | Роль |
|---|---|---|---|
| `svelte` | 5.57.1 | MIT | Фреймворк интерфейса (компилируется в сборку) |
| `@tauri-apps/api` | 2.12.0 | Apache-2.0 OR MIT | IPC |
| `@tauri-apps/plugin-dialog` | 2.8.1 | MIT OR Apache-2.0 | Системные диалоги |
| `vite` | 5.4.21 | MIT | Сборщик (только на этапе сборки) |
| `typescript` | 5.9.3 | Apache-2.0 | Компилятор (только на этапе сборки) |
| `svelte-check`, `@sveltejs/vite-plugin-svelte`, `@tsconfig/svelte`, `tslib` | — | MIT, 0BSD | Только на этапе сборки |

## 5. Встроенные ресурсы и данные третьих сторон

| Ресурс | Лицензия | Статус в поставке |
|---|---|---|
| lottie-web (`ui/public/vendor/lottie_light_canvas.min.js`) | MIT, © 2015 Bodymovin | Включён; текст лицензии — `ui/public/vendor/lottie-LICENSE.md` |
| emojibase-data (источник `emojiData.ts`) | MIT | Производные данные включены |
| Словарь BIP-0039 English (`bip39_english.txt`, SHA-256 `2f5eed53…`) | Документ BIP-0039 отдельной лицензии не указывает; словарь общеупотребителен в реализациях под MIT и Apache-2.0 | Включён без изменений |
| Шрифты Onest, JetBrains Mono | SIL Open Font License 1.1 | Не включены; загружаются с Google Fonts |
| Набор эмодзи «El Taller» (`eltaller47`) и иные наборы Telegram | Права принадлежат авторам наборов | Не включены; загружаются ретранслятором по указанию администратора |
| Движки WebView (WebView2, WKWebView, WebKitGTK) | Лицензии поставщиков; WebKitGTK — LGPL-2.1 | Не включены; используются системные компоненты с динамической компоновкой |
| GStreamer, ffmpeg | LGPL-2.1+; ffmpeg — LGPL-2.1+ или GPL в зависимости от сборки | Не включены; вызываются как внешние системные компоненты |
